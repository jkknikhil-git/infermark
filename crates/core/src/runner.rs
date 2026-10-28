use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use chrono::Utc;
use infermark_sdk::{
    HardwareInfo, ModuleError, ModuleOutput, ModuleRecord, ModuleStatus, PlanStatus, Profile,
    Progress, RunContext, RunResult, SensorChannel, SensorInventory, SensorProvider, SensorReading,
    SensorSnapshot, Telemetry, SCHEMA_VERSION,
};
use infermark_telemetry::{Sample, Sampler, DEFAULT_MAX_POINTS};
use serde::Serialize;
use uuid::Uuid;

use crate::{plan, privacy, Registry};

/// Live updates while a run is in progress (forwarded to the CLI progress bar or the app).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RunEvent {
    /// First event of every run: which sensors this device has (and which it doesn't).
    RunStarted {
        sensors: SensorInventory,
    },
    /// One sensor sample, about once a second; `t` is seconds since the run started.
    Sensors {
        t: f64,
        readings: Vec<SensorReading>,
    },
    ModuleStarted {
        module_id: String,
        index: usize,
        total: usize,
    },
    ModuleProgress {
        module_id: String,
        progress: Progress,
    },
    ModuleFinished {
        record: ModuleRecord,
    },
}

/// How often sensors are sampled during a run.
pub const TELEMETRY_INTERVAL: Duration = Duration::from_secs(1);

/// Plan and run a profile. Skipped modules are recorded with their reason; a failing module
/// doesn't stop the run. Setting `cancel` stops after the current module. Sensors available on
/// this platform are sampled in the background for the whole run.
pub fn run(
    profile: &Profile,
    registry: &Registry,
    hardware: &HardwareInfo,
    on_event: &(dyn Fn(RunEvent) + Send + Sync),
    cancel: &AtomicBool,
) -> RunResult {
    run_with_sensors(
        profile,
        registry,
        hardware,
        on_event,
        cancel,
        infermark_telemetry::default_providers(),
    )
}

/// Like [`run`], with explicit sensor providers (phones bring their own; tests pass none).
pub fn run_with_sensors(
    profile: &Profile,
    registry: &Registry,
    hardware: &HardwareInfo,
    on_event: &(dyn Fn(RunEvent) + Send + Sync),
    cancel: &AtomicBool,
    sensors: Vec<Box<dyn SensorProvider>>,
) -> RunResult {
    let channels: Vec<SensorChannel> = sensors.iter().flat_map(|p| p.channels()).collect();
    on_event(RunEvent::RunStarted {
        sensors: SensorInventory::from_channels(channels),
    });

    // The sampler thread needs a 'static callback but `on_event` is borrowed, so samples go
    // through a channel to a scoped forwarder. The channel closes when the sampler stops.
    let snapshot = Arc::new(SensorSnapshot::default());
    let (tx, rx) = mpsc::channel::<Sample>();
    let latest = snapshot.clone();
    let sampler = Sampler::start(
        sensors,
        TELEMETRY_INTERVAL,
        DEFAULT_MAX_POINTS,
        Some(Box::new(move |s: &Sample| {
            latest.set(s.readings.clone());
            let _ = tx.send(s.clone());
        })),
    );
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for s in rx {
                on_event(RunEvent::Sensors {
                    t: s.t,
                    readings: s.readings,
                });
            }
        });
        let result = run_modules(profile, registry, hardware, on_event, cancel, &snapshot);
        let recording = sampler.stop();
        RunResult {
            telemetry: Some(Telemetry {
                sensors: recording.inventory,
                interval_ms: recording.interval_ms,
                series: recording.series,
            }),
            ..result
        }
    })
}

/// Plan and run every module; telemetry is filled in by the caller.
fn run_modules(
    profile: &Profile,
    registry: &Registry,
    hardware: &HardwareInfo,
    on_event: &(dyn Fn(RunEvent) + Send + Sync),
    cancel: &AtomicBool,
    sensors: &SensorSnapshot,
) -> RunResult {
    let run_start = Instant::now();
    let started_at = Utc::now();
    let plan = plan(profile, registry, hardware);
    let total = plan.len();
    let mut records = Vec::with_capacity(total);

    for (index, entry) in plan.iter().enumerate() {
        let id = entry.module_id.clone();
        on_event(RunEvent::ModuleStarted {
            module_id: id.clone(),
            index,
            total,
        });
        let start = Instant::now();
        let offset_ms = start.duration_since(run_start).as_secs_f64() * 1000.0;

        let (status, output) = match (&entry.status, registry.module(&id)) {
            (PlanStatus::Skipped { reason }, _) => (
                ModuleStatus::Skipped {
                    reason: reason.clone(),
                },
                ModuleOutput::default(),
            ),
            _ if cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                (ModuleStatus::Cancelled, ModuleOutput::default())
            }
            (PlanStatus::Planned, Some(module)) => {
                let empty = toml::Table::new();
                let settings = profile
                    .modules
                    .get(&id)
                    .map(|m| &m.settings)
                    .unwrap_or(&empty);
                let progress_id = id.clone();
                let progress = move |p: Progress| {
                    on_event(RunEvent::ModuleProgress {
                        module_id: progress_id.clone(),
                        progress: p,
                    })
                };
                let ctx = RunContext {
                    hardware,
                    settings,
                    run: &profile.run,
                    profile,
                    earlier: &records,
                    progress: &progress,
                    cancel,
                    sensors,
                };
                match module.run(&ctx) {
                    Ok(output) => (ModuleStatus::Completed, output),
                    Err(ModuleError::Cancelled) => {
                        (ModuleStatus::Cancelled, ModuleOutput::default())
                    }
                    Err(ModuleError::Skipped(reason)) => {
                        (ModuleStatus::Skipped { reason }, ModuleOutput::default())
                    }
                    Err(e) => (
                        ModuleStatus::Failed {
                            error: e.to_string(),
                        },
                        ModuleOutput::default(),
                    ),
                }
            }
            (PlanStatus::Planned, None) => unreachable!("planner only plans registered modules"),
        };

        // Engine messages can contain local paths (and so the account name): strip them.
        let status = match status {
            ModuleStatus::Failed { error } => ModuleStatus::Failed {
                error: privacy::redact(&error),
            },
            ModuleStatus::Skipped { reason } => ModuleStatus::Skipped {
                reason: privacy::redact(&reason),
            },
            other => other,
        };
        let mut output = output;
        for note in &mut output.notes {
            *note = privacy::redact(note);
        }
        let record = ModuleRecord {
            module_id: id,
            status,
            offset_ms,
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
            output,
        };
        on_event(RunEvent::ModuleFinished {
            record: record.clone(),
        });
        records.push(record);
    }

    RunResult {
        telemetry: None,
        schema: SCHEMA_VERSION,
        infermark_version: crate::VERSION.to_string(),
        run_id: Uuid::new_v4(),
        started_at,
        finished_at: Utc::now(),
        hardware: hardware.clone(),
        profile: profile.clone(),
        plan,
        modules: records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use infermark_sdk::SensorKind;
    use std::sync::Mutex;

    struct Load;

    impl SensorProvider for Load {
        fn id(&self) -> &'static str {
            "load"
        }
        fn channels(&self) -> Vec<SensorChannel> {
            vec![SensorChannel::new(
                "cpu",
                SensorKind::Utilization,
                "CPU load",
            )]
        }
        fn sample(&mut self) -> Vec<SensorReading> {
            vec![SensorReading::new(
                "cpu",
                SensorKind::Utilization,
                Some(50.0),
            )]
        }
    }

    #[test]
    fn sensor_samples_become_run_events_and_the_run_still_ends() {
        let profile = Profile::from_toml("schema = 1\nname = \"t\"\n").unwrap();
        let hw: HardwareInfo = serde_json::from_value(serde_json::json!({
            "fingerprint": "t",
            "os": {"family": "linux", "name": "L", "version": "1", "kernel": "", "arch": "x86_64"},
            "cpu": {"brand": "c", "vendor": "", "physical_cores": 4, "logical_cores": 8, "frequency_mhz": null},
            "memory": {"total_bytes": 8589934592u64, "available_bytes": 4294967296u64},
            "gpus": [], "npus": [], "battery": null, "power_source": "ac"
        }))
        .unwrap();
        let events = Mutex::new(Vec::new());
        let on_event = |e: RunEvent| events.lock().unwrap().push(e);
        let result = run_with_sensors(
            &profile,
            &Registry::empty(),
            &hw,
            &on_event,
            &AtomicBool::new(false),
            vec![Box::new(Load)],
        );
        let events = events.into_inner().unwrap();
        match &events[0] {
            RunEvent::RunStarted { sensors } => {
                assert!(sensors
                    .available
                    .iter()
                    .any(|c| c.key() == "cpu.utilization"));
                assert!(sensors.missing.iter().any(|c| c.key() == "cpu.temperature"));
            }
            other => panic!("first event was {other:?}"),
        }
        // The sampler takes its first sample immediately.
        assert!(events.iter().any(
            |e| matches!(e, RunEvent::Sensors { readings, .. } if readings[0].value == Some(50.0))
        ));
        assert_eq!(result.telemetry.unwrap().series[0].key, "cpu.utilization");
    }
}
