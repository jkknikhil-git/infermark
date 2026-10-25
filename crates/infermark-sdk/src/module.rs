//! Benchmark modules: the unit of work the planner schedules.

use std::sync::atomic::{AtomicBool, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::hardware::{GpuVendor, HardwareInfo, OsFamily, PowerSource};

/// A benchmark that can be planned and run.
///
/// Implementations must be cheap to construct; heavy setup belongs in [`BenchModule::run`].
pub trait BenchModule: Send + Sync {
    fn descriptor(&self) -> ModuleDescriptor;

    /// Run the benchmark. Long-running modules should call `ctx.report` regularly and stop
    /// early (returning [`ModuleError::Cancelled`]) when `ctx.is_cancelled()` becomes true.
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ModuleDescriptor {
    /// Stable kebab-case id used in profiles, e.g. `memory-bandwidth`.
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: ModuleCategory,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    /// Rough duration estimate with default settings, shown in the run wizard.
    pub estimated_seconds: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleCategory {
    System,
    Microbenchmark,
    Inference,
    Endurance,
    Battery,
    Quality,
}

/// Something a module needs from the hardware or environment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Requirement {
    /// At least this much total system RAM.
    MinRamGb { gb: f64 },
    /// At least this much dedicated VRAM on some GPU.
    MinVramGb { gb: f64 },
    /// Any GPU at all (integrated counts).
    AnyGpu,
    /// A GPU from a specific vendor (e.g. NVML-based telemetry needs NVIDIA).
    GpuVendor { vendor: GpuVendor },
    /// A battery must be present.
    Battery,
    /// Must be running on battery power (battery drain tests).
    OnBatteryPower,
    /// Only on these operating systems.
    Os { families: Vec<OsFamily> },
}

impl Requirement {
    /// `Ok(())` if satisfied, otherwise a human-readable reason it isn't.
    pub fn check(&self, hw: &HardwareInfo) -> Result<(), String> {
        match self {
            Self::MinRamGb { gb } => {
                let have = hw.total_ram_gb();
                if have + 0.25 >= *gb {
                    Ok(())
                } else {
                    Err(format!(
                        "needs {gb:.0} GB RAM, this device has {have:.1} GB"
                    ))
                }
            }
            Self::MinVramGb { gb } => {
                let have = hw.max_dedicated_vram_bytes() as f64 / (1024.0 * 1024.0 * 1024.0);
                if have + 0.25 >= *gb {
                    Ok(())
                } else {
                    Err(format!(
                        "needs {gb:.0} GB of GPU memory, this device has {have:.1} GB"
                    ))
                }
            }
            Self::AnyGpu => {
                if hw.gpus.is_empty() {
                    Err("needs a GPU, none was detected".into())
                } else {
                    Ok(())
                }
            }
            Self::GpuVendor { vendor } => {
                if hw.has_gpu_vendor(*vendor) {
                    Ok(())
                } else {
                    Err(format!("needs a {} GPU", vendor_label(*vendor)))
                }
            }
            Self::Battery => {
                if hw.battery.is_some() {
                    Ok(())
                } else {
                    Err("needs a battery (desktop detected)".into())
                }
            }
            Self::OnBatteryPower => match hw.power_source {
                PowerSource::Battery => Ok(()),
                PowerSource::Ac => Err("unplug the charger to run battery tests".into()),
                PowerSource::Unknown => Err("power source could not be detected".into()),
            },
            Self::Os { families } => {
                if families.contains(&hw.os.family) {
                    Ok(())
                } else {
                    Err(format!("not supported on {:?}", hw.os.family))
                }
            }
        }
    }
}

fn vendor_label(v: GpuVendor) -> &'static str {
    match v {
        GpuVendor::Nvidia => "NVIDIA",
        GpuVendor::Amd => "AMD",
        GpuVendor::Intel => "Intel",
        GpuVendor::Apple => "Apple",
        GpuVendor::Qualcomm => "Qualcomm",
        GpuVendor::Arm => "Arm",
        GpuVendor::Microsoft => "Microsoft",
        GpuVendor::Other => "compatible",
    }
}

/// Everything a module gets while running.
pub struct RunContext<'a> {
    pub hardware: &'a HardwareInfo,
    /// Module-specific settings from the profile (`[modules.<id>]` table minus `enabled`).
    pub settings: &'a toml::Table,
    /// Global run settings from the profile.
    pub run: &'a crate::profile::RunSettings,
    pub progress: &'a (dyn Fn(Progress) + Send + Sync),
    pub cancel: &'a AtomicBool,
}

impl RunContext<'_> {
    pub fn report(&self, fraction: f64, message: impl Into<String>) {
        (self.progress)(Progress {
            fraction: fraction.clamp(0.0, 1.0),
            message: message.into(),
        });
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn setting_u64(&self, key: &str, default: u64) -> u64 {
        self.settings
            .get(key)
            .and_then(|v| v.as_integer())
            .and_then(|v| u64::try_from(v).ok())
            .unwrap_or(default)
    }

    pub fn setting_f64(&self, key: &str, default: f64) -> f64 {
        self.settings
            .get(key)
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            .unwrap_or(default)
    }

    pub fn setting_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Progress {
    pub fraction: f64,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ModuleOutput {
    #[serde(default)]
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub series: Vec<Series>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// A single headline number.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Metric {
    /// Stable key, e.g. `triad_gbps`.
    pub key: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub direction: Direction,
}

impl Metric {
    pub fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        unit: impl Into<String>,
        direction: Direction,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value,
            unit: unit.into(),
            direction,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    HigherIsBetter,
    LowerIsBetter,
    Neutral,
}

/// A time series (or any x/y series) for graphs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Series {
    pub key: String,
    pub label: String,
    pub x_unit: String,
    pub y_unit: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("cancelled")]
    Cancelled,
    #[error("invalid setting `{key}`: {message}")]
    InvalidSetting { key: String, message: String },
    #[error("{0}")]
    Failed(String),
}
