//! Sensor providers for Windows, macOS and Linux.

use std::path::{Path, PathBuf};
use std::time::Instant;

use infermark_hwprobe::read_battery;
use infermark_sdk::{SensorChannel, SensorKind, SensorProvider, SensorReading};
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;
use sysinfo::{Components, CpuRefreshKind, System};

/// CPU load and clock from the OS; CPU temperature where the OS exposes it to normal users
/// (Linux usually does; Windows usually only with admin rights, so it is reported missing).
pub struct CpuProvider {
    sys: System,
    components: Components,
    temp_component: Option<usize>,
    primed: bool,
}

impl CpuProvider {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        let components = Components::new_with_refreshed_list();
        let temp_component = cpu_temperature_component(&components);
        Self {
            sys,
            components,
            temp_component,
            primed: false,
        }
    }

    fn has_clock(&self) -> bool {
        self.sys.cpus().iter().any(|c| c.frequency() > 0)
    }
}

impl Default for CpuProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// The component that best represents the CPU package, if any.
fn cpu_temperature_component(components: &Components) -> Option<usize> {
    let labels: Vec<String> = components
        .iter()
        .map(|c| c.label().to_lowercase())
        .collect();
    let usable = |i: usize| {
        components
            .get(i)
            .and_then(|c| c.temperature())
            .is_some_and(|t| t > 0.0)
    };
    let find = |needles: &[&str]| {
        labels
            .iter()
            .enumerate()
            .find(|(i, l)| needles.iter().any(|n| l.contains(n)) && usable(*i))
            .map(|(i, _)| i)
    };
    find(&["package", "tctl", "tdie"]).or_else(|| find(&["coretemp", "k10temp", "cpu"]))
}

impl SensorProvider for CpuProvider {
    fn id(&self) -> &'static str {
        "cpu"
    }

    fn channels(&self) -> Vec<SensorChannel> {
        let mut c = vec![SensorChannel::new(
            "cpu",
            SensorKind::Utilization,
            "CPU load",
        )];
        if self.has_clock() {
            c.push(SensorChannel::new("cpu", SensorKind::Clock, "CPU clock"));
        }
        if self.temp_component.is_some() {
            c.push(SensorChannel::new(
                "cpu",
                SensorKind::Temperature,
                "CPU temperature",
            ));
        }
        c
    }

    fn sample(&mut self) -> Vec<SensorReading> {
        self.sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage().with_frequency());
        // CPU load needs two refreshes to mean anything; the first sample has none.
        let load = self.primed.then(|| self.sys.global_cpu_usage() as f64);
        self.primed = true;
        let freqs: Vec<f64> = self
            .sys
            .cpus()
            .iter()
            .map(|c| c.frequency() as f64)
            .filter(|f| *f > 0.0)
            .collect();
        let clock = (!freqs.is_empty()).then(|| freqs.iter().sum::<f64>() / freqs.len() as f64);

        let mut out = vec![SensorReading::new("cpu", SensorKind::Utilization, load)];
        if self.has_clock() {
            out.push(SensorReading::new("cpu", SensorKind::Clock, clock));
        }
        if let Some(i) = self.temp_component {
            self.components.refresh(false);
            let t = self
                .components
                .get(i)
                .and_then(|c| c.temperature())
                .map(f64::from);
            out.push(SensorReading::new("cpu", SensorKind::Temperature, t));
        }
        out
    }
}

/// NVIDIA GPUs through NVML (ships with the driver): load, temperature, power, clock.
pub struct NvidiaProvider {
    nvml: Option<Nvml>,
    count: u32,
}

impl NvidiaProvider {
    pub fn new() -> Self {
        let nvml = Nvml::init().ok();
        let count = nvml
            .as_ref()
            .and_then(|n| n.device_count().ok())
            .unwrap_or(0);
        Self { nvml, count }
    }
}

impl Default for NvidiaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorProvider for NvidiaProvider {
    fn id(&self) -> &'static str {
        "nvidia"
    }

    fn channels(&self) -> Vec<SensorChannel> {
        (0..self.count)
            .flat_map(|i| {
                let src = format!("gpu{i}");
                let n = if self.count > 1 {
                    format!(" {}", i + 1)
                } else {
                    String::new()
                };
                [
                    SensorChannel::new(&src, SensorKind::Utilization, format!("GPU{n} load")),
                    SensorChannel::new(
                        &src,
                        SensorKind::Temperature,
                        format!("GPU{n} temperature"),
                    ),
                    SensorChannel::new(&src, SensorKind::Power, format!("GPU{n} power")),
                    SensorChannel::new(&src, SensorKind::Clock, format!("GPU{n} clock")),
                ]
            })
            .collect()
    }

    fn sample(&mut self) -> Vec<SensorReading> {
        let Some(nvml) = &self.nvml else {
            return Vec::new();
        };
        (0..self.count)
            .flat_map(|i| {
                let src = format!("gpu{i}");
                let dev = nvml.device_by_index(i).ok();
                let load = dev
                    .as_ref()
                    .and_then(|d| d.utilization_rates().ok())
                    .map(|u| u.gpu as f64);
                let temp = dev
                    .as_ref()
                    .and_then(|d| d.temperature(TemperatureSensor::Gpu).ok())
                    .map(f64::from);
                let power = dev
                    .as_ref()
                    .and_then(|d| d.power_usage().ok())
                    .map(|mw| mw as f64 / 1000.0);
                let clock = dev
                    .as_ref()
                    .and_then(|d| d.clock_info(Clock::Graphics).ok())
                    .map(f64::from);
                [
                    SensorReading::new(&src, SensorKind::Utilization, load),
                    SensorReading::new(&src, SensorKind::Temperature, temp),
                    SensorReading::new(&src, SensorKind::Power, power),
                    SensorReading::new(&src, SensorKind::Clock, clock),
                ]
            })
            .collect()
    }
}

/// Laptop battery: charge, drain in watts while on battery, and temperature where reported.
pub struct BatteryProvider {
    present: bool,
    has_temperature: bool,
}

impl BatteryProvider {
    pub fn new() -> Self {
        let first = read_battery();
        Self {
            present: first.is_some(),
            has_temperature: first.is_some_and(|b| b.temperature_c.is_some()),
        }
    }
}

impl Default for BatteryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SensorProvider for BatteryProvider {
    fn id(&self) -> &'static str {
        "battery"
    }

    fn channels(&self) -> Vec<SensorChannel> {
        if !self.present {
            return Vec::new();
        }
        let mut c = vec![
            SensorChannel::new("battery", SensorKind::BatteryCharge, "Battery charge"),
            SensorChannel::new("battery", SensorKind::Power, "Battery drain"),
        ];
        if self.has_temperature {
            c.push(SensorChannel::new(
                "battery",
                SensorKind::Temperature,
                "Battery temperature",
            ));
        }
        c
    }

    fn sample(&mut self) -> Vec<SensorReading> {
        if !self.present {
            return Vec::new();
        }
        let now = read_battery();
        let mut out = vec![
            SensorReading::new(
                "battery",
                SensorKind::BatteryCharge,
                now.as_ref().map(|b| b.charge_percent),
            ),
            SensorReading::new(
                "battery",
                SensorKind::Power,
                now.as_ref().and_then(|b| b.discharge_w),
            ),
        ];
        if self.has_temperature {
            out.push(SensorReading::new(
                "battery",
                SensorKind::Temperature,
                now.as_ref().and_then(|b| b.temperature_c),
            ));
        }
        out
    }
}

/// CPU package power from Linux RAPL (`/sys/class/powercap/intel-rapl:N`, also used by AMD).
/// Most distributions make `energy_uj` readable by root only, and Windows and macOS have no
/// equivalent without a driver or an admin tool, so there it is reported missing.
pub struct PackagePowerProvider {
    zones: Vec<RaplZone>,
    /// Energy counters and time of the previous sample.
    last: Option<(Vec<u64>, Instant)>,
}

struct RaplZone {
    energy: PathBuf,
    /// The counter wraps back to zero after this many µJ.
    max_range: Option<u64>,
}

impl PackagePowerProvider {
    pub fn new() -> Self {
        Self::from_powercap(Path::new("/sys/class/powercap"))
    }

    /// Package zones under `root` (`intel-rapl:0`, `intel-rapl:1`, ... whose `name` starts
    /// with "package") whose energy counter this process can read.
    fn from_powercap(root: &Path) -> Self {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.sort();
        let zones = dirs
            .into_iter()
            .filter(|d| {
                let name = d.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Top-level zones only; `intel-rapl:0:0` is a sub-zone (cores, uncore, dram).
                name.starts_with("intel-rapl:") && name.matches(':').count() == 1
            })
            .filter(|d| {
                std::fs::read_to_string(d.join("name"))
                    .is_ok_and(|n| n.trim().starts_with("package"))
            })
            .filter(|d| read_u64(&d.join("energy_uj")).is_some())
            .map(|d| RaplZone {
                max_range: read_u64(&d.join("max_energy_range_uj")).filter(|m| *m > 0),
                energy: d.join("energy_uj"),
            })
            .collect();
        Self { zones, last: None }
    }
}

impl Default for PackagePowerProvider {
    fn default() -> Self {
        Self::new()
    }
}

fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Average watts between two energy counter readings `secs` apart, allowing for one wrap.
fn rapl_watts(prev: u64, now: u64, max_range: Option<u64>, secs: f64) -> Option<f64> {
    if secs <= 0.0 {
        return None;
    }
    let delta = if now >= prev {
        now - prev
    } else {
        max_range?.checked_sub(prev)?.checked_add(now)?
    };
    Some(delta as f64 / 1e6 / secs)
}

impl SensorProvider for PackagePowerProvider {
    fn id(&self) -> &'static str {
        "rapl"
    }

    fn channels(&self) -> Vec<SensorChannel> {
        if self.zones.is_empty() {
            return Vec::new();
        }
        vec![SensorChannel::new(
            "cpu",
            SensorKind::Power,
            "CPU package power",
        )]
    }

    fn sample(&mut self) -> Vec<SensorReading> {
        if self.zones.is_empty() {
            return Vec::new();
        }
        let at = Instant::now();
        let now: Option<Vec<u64>> = self.zones.iter().map(|z| read_u64(&z.energy)).collect();
        // Power needs two readings; the first sample (or one after a failed read) has none.
        let watts = match (&self.last, &now) {
            (Some((prev, then)), Some(now)) => {
                let secs = at.duration_since(*then).as_secs_f64();
                self.zones
                    .iter()
                    .zip(prev.iter().zip(now))
                    .map(|(z, (p, n))| rapl_watts(*p, *n, z.max_range, secs))
                    .sum::<Option<f64>>()
            }
            _ => None,
        };
        self.last = now.map(|n| (n, at));
        vec![SensorReading::new("cpu", SensorKind::Power, watts)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn zone(root: &Path, dir: &str, name: &str, energy: Option<u64>) {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("name"), format!("{name}\n")).unwrap();
        std::fs::write(d.join("max_energy_range_uj"), "262143328850\n").unwrap();
        if let Some(e) = energy {
            std::fs::write(d.join("energy_uj"), format!("{e}\n")).unwrap();
        }
    }

    #[test]
    fn rapl_watts_handles_wraparound() {
        assert_eq!(rapl_watts(1_000_000, 11_000_000, None, 1.0), Some(10.0));
        assert_eq!(
            rapl_watts(99_000_000, 1_000_000, Some(100_000_000), 1.0),
            Some(2.0)
        );
        assert_eq!(
            rapl_watts(90, 10, None, 1.0),
            None,
            "can't tell how far it wrapped"
        );
        assert_eq!(rapl_watts(0, 10, None, 0.0), None);
    }

    // Zone directories are named `intel-rapl:0`, which Windows can't create.
    #[cfg(unix)]
    #[test]
    fn package_power_only_from_readable_package_zones() {
        let dir = tempfile::tempdir().unwrap();
        zone(dir.path(), "intel-rapl:0", "package-0", Some(5_000_000));
        zone(dir.path(), "intel-rapl:0:0", "core", Some(1_000_000));
        zone(dir.path(), "intel-rapl:1", "psys", Some(9_000_000));
        let mut p = PackagePowerProvider::from_powercap(dir.path());
        assert_eq!(p.zones.len(), 1);
        assert_eq!(p.channels().len(), 1);
        assert_eq!(p.channels()[0].key(), "cpu.power");

        let first = p.sample();
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].value, None,
            "the first sample has no previous reading"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.path().join("intel-rapl:0/energy_uj"), "5500000\n").unwrap();
        let w = p.sample()[0].value.unwrap();
        assert!(w > 0.0 && w.is_finite(), "{w}");

        // The counter becomes unreadable (e.g. permissions): no value, not zero.
        std::fs::remove_file(dir.path().join("intel-rapl:0/energy_uj")).unwrap();
        assert_eq!(p.sample()[0].value, None);
    }

    #[cfg(unix)]
    #[test]
    fn root_only_counter_means_no_channel() {
        let dir = tempfile::tempdir().unwrap();
        // Stands in for a root-only `energy_uj`: the zone exists but can't be read.
        zone(dir.path(), "intel-rapl:0", "package-0", None);
        let mut p = PackagePowerProvider::from_powercap(dir.path());
        assert!(p.channels().is_empty());
        assert!(p.sample().is_empty());
    }

    #[test]
    fn no_powercap_means_no_channel() {
        let dir = tempfile::tempdir().unwrap();
        let p = PackagePowerProvider::from_powercap(&dir.path().join("nope"));
        assert!(p.channels().is_empty());
    }
}
