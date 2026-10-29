//! Background telemetry while a benchmark runs.
//!
//! ```text
//!  providers ──► Sampler thread (every interval) ──► on_sample callback (live charts)
//!                     │
//!                     └─► bounded series ──► Recording { inventory, series }  (saved in results)
//! ```
//!
//! Providers only offer the channels that really exist on this device; anything else is listed
//! as missing in the [`SensorInventory`], never recorded as zeros.

mod desktop;

use std::collections::BTreeMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use infermark_sdk::{SensorChannel, SensorInventory, SensorProvider, SensorReading, Series};

#[cfg(not(target_os = "android"))]
pub use desktop::NvidiaProvider;
pub use desktop::{BatteryProvider, CpuProvider, PackagePowerProvider};

/// Default points kept per series; long runs are thinned out evenly beyond this.
pub const DEFAULT_MAX_POINTS: usize = 1200;

/// Sensors available on this platform (desktop: CPU, CPU package power, NVIDIA GPUs, battery).
pub fn default_providers() -> Vec<Box<dyn SensorProvider>> {
    let all: Vec<Box<dyn SensorProvider>> = vec![
        Box::new(CpuProvider::new()),
        Box::new(PackagePowerProvider::new()),
        #[cfg(not(target_os = "android"))]
        Box::new(NvidiaProvider::new()),
        Box::new(BatteryProvider::new()),
    ];
    all.into_iter()
        .filter(|p| !p.channels().is_empty())
        .collect()
}

/// Called on the sampler thread after every tick (for live charts).
pub type OnSample = Box<dyn Fn(&Sample) + Send>;

/// One tick of readings, `t` seconds after the sampler started.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub readings: Vec<SensorReading>,
}

/// What a finished sampler hands back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recording {
    pub inventory: SensorInventory,
    pub interval_ms: u64,
    /// One series per channel that produced at least one value, keyed `source.kind`.
    pub series: Vec<Series>,
}

pub struct Sampler {
    stop: Sender<()>,
    handle: JoinHandle<Recording>,
}

impl Sampler {
    /// Start sampling on a background thread. Providers with no channels are ignored.
    pub fn start(
        providers: Vec<Box<dyn SensorProvider>>,
        interval: Duration,
        max_points: usize,
        on_sample: Option<OnSample>,
    ) -> Self {
        let (stop, rx) = mpsc::channel::<()>();
        let handle = std::thread::Builder::new()
            .name("infermark-telemetry".into())
            .spawn(move || {
                let mut providers: Vec<_> = providers
                    .into_iter()
                    .filter(|p| !p.channels().is_empty())
                    .collect();
                let channels: Vec<SensorChannel> =
                    providers.iter().flat_map(|p| p.channels()).collect();
                let mut store = SeriesStore::new(&channels, max_points.max(2));
                let start = Instant::now();
                loop {
                    let t = start.elapsed().as_secs_f64();
                    let readings: Vec<SensorReading> =
                        providers.iter_mut().flat_map(|p| p.sample()).collect();
                    let sample = Sample { t, readings };
                    store.push(&sample);
                    if let Some(cb) = &on_sample {
                        cb(&sample);
                    }
                    match rx.recv_timeout(interval) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        // Stop requested, or the Sampler was dropped.
                        _ => break,
                    }
                }
                Recording {
                    inventory: SensorInventory::from_channels(channels),
                    interval_ms: interval.as_millis() as u64,
                    series: store.finish(),
                }
            })
            .expect("spawn telemetry thread");
        Self { stop, handle }
    }

    /// Stop sampling and return everything recorded.
    pub fn stop(self) -> Recording {
        let _ = self.stop.send(());
        self.handle.join().unwrap_or_default()
    }
}

/// Per-channel points with a hard cap: when a series is full, every other point is dropped and
/// new points are kept at half the rate, so a long run keeps its overall shape.
struct SeriesStore {
    series: BTreeMap<String, Track>,
    max_points: usize,
}

struct Track {
    channel: SensorChannel,
    points: Vec<(f64, f64)>,
    /// Keep one value out of every `stride` (doubles each time the track is thinned).
    stride: usize,
    seen: usize,
}

impl SeriesStore {
    fn new(channels: &[SensorChannel], max_points: usize) -> Self {
        Self {
            series: channels
                .iter()
                .map(|c| {
                    let track = Track {
                        channel: c.clone(),
                        points: Vec::new(),
                        stride: 1,
                        seen: 0,
                    };
                    (c.key(), track)
                })
                .collect(),
            max_points,
        }
    }

    fn push(&mut self, sample: &Sample) {
        for r in &sample.readings {
            let (Some(value), Some(track)) = (r.value, self.series.get_mut(&r.key())) else {
                continue;
            };
            track.seen += 1;
            if (track.seen - 1) % track.stride != 0 {
                continue;
            }
            track.points.push((round(sample.t, 2), round(value, 2)));
            if track.points.len() >= self.max_points {
                track.points = track.points.iter().copied().step_by(2).collect();
                track.stride *= 2;
            }
        }
    }

    fn finish(self) -> Vec<Series> {
        self.series
            .into_iter()
            .filter(|(_, track)| !track.points.is_empty())
            .map(
                |(
                    key,
                    Track {
                        channel, points, ..
                    },
                )| Series {
                    key,
                    label: channel.label.clone(),
                    x_unit: "s".into(),
                    y_unit: channel.kind.unit().into(),
                    points,
                },
            )
            .collect()
    }
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

#[cfg(test)]
mod tests {
    use super::*;
    use infermark_sdk::SensorKind;
    use std::sync::{Arc, Mutex};

    /// Has CPU load, plus a temperature channel that can't be read.
    struct Fake {
        n: u32,
    }

    impl SensorProvider for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn channels(&self) -> Vec<SensorChannel> {
            vec![
                SensorChannel::new("cpu", SensorKind::Utilization, "CPU load"),
                SensorChannel::new("cpu", SensorKind::Temperature, "CPU temperature"),
            ]
        }
        fn sample(&mut self) -> Vec<SensorReading> {
            self.n += 1;
            vec![
                SensorReading::new("cpu", SensorKind::Utilization, Some(self.n as f64)),
                SensorReading::new("cpu", SensorKind::Temperature, None),
            ]
        }
    }

    struct Nothing;

    impl SensorProvider for Nothing {
        fn id(&self) -> &'static str {
            "nothing"
        }
        fn channels(&self) -> Vec<SensorChannel> {
            vec![]
        }
        fn sample(&mut self) -> Vec<SensorReading> {
            panic!("providers without channels are never sampled")
        }
    }

    #[test]
    fn unreadable_channels_record_no_points_and_missing_ones_are_listed() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let sampler = Sampler::start(
            vec![Box::new(Fake { n: 0 }), Box::new(Nothing)],
            Duration::from_millis(5),
            100,
            Some(Box::new(move |s: &Sample| {
                sink.lock().unwrap().push(s.clone())
            })),
        );
        std::thread::sleep(Duration::from_millis(60));
        let rec = sampler.stop();

        assert_eq!(
            rec.series.len(),
            1,
            "the unreadable temperature gets no series (not zeros)"
        );
        let load = &rec.series[0];
        assert_eq!(load.key, "cpu.utilization");
        assert!(load.points.len() >= 2);
        assert!(load.points.iter().all(|(_, v)| *v > 0.0));

        // The temperature channel exists but never produced a value.
        assert!(rec
            .inventory
            .available
            .iter()
            .any(|c| c.key() == "cpu.temperature"));
        assert!(rec.inventory.missing.iter().any(|c| c.key() == "gpu.power"));
        assert!(!seen.lock().unwrap().is_empty(), "live callback was called");
        assert!(seen.lock().unwrap()[0]
            .readings
            .iter()
            .any(|r| r.kind == SensorKind::Temperature && r.value.is_none()));
    }

    #[test]
    fn long_runs_are_thinned_not_unbounded() {
        let mut store = SeriesStore::new(
            &[SensorChannel::new(
                "cpu",
                SensorKind::Utilization,
                "CPU load",
            )],
            10,
        );
        for i in 0..1000 {
            store.push(&Sample {
                t: i as f64,
                readings: vec![SensorReading::new(
                    "cpu",
                    SensorKind::Utilization,
                    Some(i as f64),
                )],
            });
        }
        let series = store.finish();
        let points = &series[0].points;
        assert!(points.len() < 10, "{}", points.len());
        assert_eq!(points[0].0, 0.0, "keeps the start");
        assert!(
            points.last().unwrap().0 > 800.0,
            "covers the whole run: {:?}",
            points.last()
        );
    }

    #[test]
    fn stop_is_prompt() {
        let sampler = Sampler::start(
            vec![Box::new(Fake { n: 0 })],
            Duration::from_secs(60),
            100,
            None,
        );
        let t = Instant::now();
        let rec = sampler.stop();
        assert!(t.elapsed() < Duration::from_secs(2));
        assert_eq!(rec.interval_ms, 60_000);
    }

    #[test]
    fn real_providers_on_this_machine() {
        // Whatever this machine has, CPU load is always available and nothing reads as NaN.
        let rec = {
            let s = Sampler::start(default_providers(), Duration::from_millis(200), 100, None);
            std::thread::sleep(Duration::from_millis(500));
            s.stop()
        };
        assert!(rec
            .inventory
            .available
            .iter()
            .any(|c| c.key() == "cpu.utilization"));
        for s in &rec.series {
            assert!(s.points.iter().all(|(_, v)| v.is_finite()), "{}", s.key);
        }
    }
}
