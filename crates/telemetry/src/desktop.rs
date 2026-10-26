//! Sensor providers for Windows, macOS and Linux.

use infermark_sdk::{SensorChannel, SensorKind, SensorProvider, SensorReading};
use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
use nvml_wrapper::Nvml;
use starship_battery::units::power::watt;
use starship_battery::units::thermodynamic_temperature::degree_celsius;
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
            has_temperature: first.is_some_and(|b| b.temperature.is_some()),
        }
    }
}

impl Default for BatteryProvider {
    fn default() -> Self {
        Self::new()
    }
}

struct BatteryNow {
    charge: f64,
    /// Watts drawn from the battery; `None` while charging or full.
    drain: Option<f64>,
    temperature: Option<f64>,
}

fn read_battery() -> Option<BatteryNow> {
    let manager = starship_battery::Manager::new().ok()?;
    let b = manager.batteries().ok()?.flatten().next()?;
    let discharging = matches!(b.state(), starship_battery::State::Discharging);
    Some(BatteryNow {
        charge: f64::from(b.state_of_charge().value) * 100.0,
        drain: discharging
            .then(|| f64::from(b.energy_rate().get::<watt>()))
            .filter(|w| *w > 0.0),
        temperature: b
            .temperature()
            .map(|t| f64::from(t.get::<degree_celsius>())),
    })
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
                now.as_ref().map(|b| b.charge),
            ),
            SensorReading::new(
                "battery",
                SensorKind::Power,
                now.as_ref().and_then(|b| b.drain),
            ),
        ];
        if self.has_temperature {
            out.push(SensorReading::new(
                "battery",
                SensorKind::Temperature,
                now.as_ref().and_then(|b| b.temperature),
            ));
        }
        out
    }
}
