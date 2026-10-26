//! Telemetry sources sampled in the background while benchmarks run.
//!
//! Devices differ wildly in what they expose: a desktop with an NVIDIA card reports GPU power
//! and temperature, a Windows laptop often has no CPU temperature without admin rights, an
//! Android phone has battery current and a thermal status, and an iPhone only a coarse thermal
//! state. So every provider declares its [`SensorChannel`]s up front, readings carry `None`
//! when a value couldn't be read, and results record a [`SensorInventory`] so the app can say
//! "not available on this device" instead of drawing a flat line at zero.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait SensorProvider: Send {
    fn id(&self) -> &'static str;

    /// The channels this provider can read on this machine. Empty means it has nothing here
    /// (no NVIDIA GPU, no battery, ...); such providers are dropped before sampling starts.
    fn channels(&self) -> Vec<SensorChannel>;

    /// One reading per channel. A channel that couldn't be read this time has `value: None`.
    fn sample(&mut self) -> Vec<SensorReading>;
}

/// Something a provider can measure, e.g. `gpu0` + [`SensorKind::Power`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SensorChannel {
    /// What is measured: `cpu`, `gpu0`, `battery`, `system`, `device`.
    pub source: String,
    pub kind: SensorKind,
    /// Human-readable, e.g. "GPU power".
    pub label: String,
}

impl SensorChannel {
    pub fn new(source: impl Into<String>, kind: SensorKind, label: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            kind,
            label: label.into(),
        }
    }

    /// Stable key used for series in results, e.g. `gpu0.power`.
    pub fn key(&self) -> String {
        format!("{}.{}", self.source, self.kind.id())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SensorReading {
    pub source: String,
    pub kind: SensorKind,
    /// `None` when the value couldn't be read this time. Never a made-up zero.
    pub value: Option<f64>,
}

impl SensorReading {
    pub fn new(source: impl Into<String>, kind: SensorKind, value: Option<f64>) -> Self {
        Self {
            source: source.into(),
            kind,
            value: value.filter(|v| v.is_finite()),
        }
    }

    pub fn key(&self) -> String {
        format!("{}.{}", self.source, self.kind.id())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum SensorKind {
    /// °C
    Temperature,
    /// MHz
    Clock,
    /// Percent 0-100
    Utilization,
    /// Watts (drawn by the component; for a battery, the discharge rate)
    Power,
    /// Bytes
    MemoryUsed,
    /// Percent 0-100
    BatteryCharge,
    /// [`ThermalState`] as 0-3. Phones and macOS report this instead of temperatures.
    ThermalState,
}

impl SensorKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Temperature => "temperature",
            Self::Clock => "clock",
            Self::Utilization => "utilization",
            Self::Power => "power",
            Self::MemoryUsed => "memory-used",
            Self::BatteryCharge => "battery-charge",
            Self::ThermalState => "thermal-state",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Temperature => "°C",
            Self::Clock => "MHz",
            Self::Utilization | Self::BatteryCharge => "%",
            Self::Power => "W",
            Self::MemoryUsed => "bytes",
            Self::ThermalState => "level",
        }
    }
}

/// Coarse thermal pressure as phones report it (Android thermal status, iOS `thermalState`).
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "kebab-case")]
pub enum ThermalState {
    Nominal = 0,
    Fair = 1,
    Serious = 2,
    Critical = 3,
}

/// The measurements a results screen wants to show, whether or not a device has them.
pub const STANDARD_CHANNELS: &[(&str, SensorKind, &str)] = &[
    ("cpu", SensorKind::Utilization, "CPU load"),
    ("cpu", SensorKind::Clock, "CPU clock"),
    ("cpu", SensorKind::Temperature, "CPU temperature"),
    ("gpu", SensorKind::Utilization, "GPU load"),
    ("gpu", SensorKind::Temperature, "GPU temperature"),
    ("gpu", SensorKind::Power, "GPU power"),
    ("battery", SensorKind::BatteryCharge, "Battery charge"),
    ("battery", SensorKind::Power, "Battery drain"),
    ("device", SensorKind::ThermalState, "Thermal state"),
];

/// Which sensors existed during a run (recorded in results).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SensorInventory {
    pub available: Vec<SensorChannel>,
    /// Standard measurements this device doesn't offer, so the app can say so.
    pub missing: Vec<SensorChannel>,
}

impl SensorInventory {
    /// Build from the channels the providers offer. `gpu0`, `gpu1`, ... count as `gpu`.
    pub fn from_channels(available: Vec<SensorChannel>) -> Self {
        let family = |source: &str| {
            source
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .to_string()
        };
        let missing = STANDARD_CHANNELS
            .iter()
            .filter(|(src, kind, _)| {
                !available
                    .iter()
                    .any(|c| c.kind == *kind && family(&c.source) == *src)
            })
            .map(|(src, kind, label)| SensorChannel::new(*src, *kind, *label))
            .collect();
        Self { available, missing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_lists_what_is_missing() {
        let inv = SensorInventory::from_channels(vec![
            SensorChannel::new("cpu", SensorKind::Utilization, "CPU load"),
            SensorChannel::new("gpu0", SensorKind::Temperature, "GPU temperature"),
        ]);
        assert!(!inv.missing.iter().any(|c| c.key() == "cpu.utilization"));
        assert!(!inv
            .missing
            .iter()
            .any(|c| c.source == "gpu" && c.kind == SensorKind::Temperature));
        assert!(inv.missing.iter().any(|c| c.key() == "cpu.temperature"));
        assert!(inv
            .missing
            .iter()
            .any(|c| c.key() == "device.thermal-state"));
        assert_eq!(
            inv.available.len() + inv.missing.len(),
            STANDARD_CHANNELS.len()
        );
    }

    #[test]
    fn readings_never_store_nan() {
        let r = SensorReading::new("cpu", SensorKind::Temperature, Some(f64::NAN));
        assert_eq!(r.value, None);
        assert_eq!(r.key(), "cpu.temperature");
    }
}
