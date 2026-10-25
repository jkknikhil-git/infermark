//! Telemetry sources sampled in the background while benchmarks run.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub trait SensorProvider: Send {
    fn id(&self) -> &'static str;

    /// Whether this provider works on the current machine. Unavailable sensors are reported as
    /// missing in results, never as zeros.
    fn available(&self) -> bool;

    fn sample(&mut self) -> Vec<SensorReading>;
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct SensorReading {
    /// e.g. `cpu.package`, `gpu0.core`.
    pub source: String,
    pub kind: SensorKind,
    pub value: f64,
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
    /// Watts
    Power,
    /// Bytes
    MemoryUsed,
    /// Percent 0-100
    BatteryCharge,
}
