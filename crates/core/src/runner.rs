use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use chrono::Utc;
use infermark_sdk::{
    HardwareInfo, ModuleError, ModuleOutput, ModuleRecord, ModuleStatus, PlanStatus, Profile,
    Progress, RunContext, RunResult, SensorProvider, Telemetry, SCHEMA_VERSION,
};
use infermark_telemetry::{Sampler, DEFAULT_MAX_POINTS};
use serde::Serialize;
use uuid::Uuid;

use crate::{plan, privacy, Registry};

/// Live updates while a run is in progress (forwarded to the CLI progress bar or the app).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RunEvent {
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
    let sampler = Sampler::start(sensors, TELEMETRY_INTERVAL, DEFAULT_MAX_POINTS, None);
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
            duration_ms: start.elapsed().as_secs_f64() * 1000.0,
            output,
        };
        on_event(RunEvent::ModuleFinished {
            record: record.clone(),
        });
        records.push(record);
    }

    let recording = sampler.stop();
    RunResult {
        telemetry: Some(Telemetry {
            sensors: recording.inventory,
            interval_ms: recording.interval_ms,
            series: recording.series,
        }),
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
