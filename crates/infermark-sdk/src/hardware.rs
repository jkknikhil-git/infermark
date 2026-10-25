//! Description of the machine a benchmark runs on.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct HardwareInfo {
    /// Stable, anonymous hash of the hardware configuration (same machine -> same fingerprint).
    pub fingerprint: String,
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    #[serde(default)]
    pub gpus: Vec<GpuInfo>,
    #[serde(default)]
    pub npus: Vec<NpuInfo>,
    /// `None` when the device has no battery (desktops).
    pub battery: Option<BatteryInfo>,
    pub power_source: PowerSource,
}

impl HardwareInfo {
    pub fn total_ram_gb(&self) -> f64 {
        self.memory.total_bytes as f64 / GIB
    }

    pub fn has_gpu_vendor(&self, vendor: GpuVendor) -> bool {
        self.gpus.iter().any(|g| g.vendor == vendor)
    }

    /// Largest dedicated VRAM across discrete GPUs, in bytes (0 for integrated-only systems,
    /// whose small "dedicated" carve-out isn't usable for models).
    pub fn max_dedicated_vram_bytes(&self) -> u64 {
        self.gpus
            .iter()
            .filter(|g| !g.integrated)
            .filter_map(|g| g.dedicated_vram_bytes)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum OsFamily {
    Windows,
    Macos,
    Linux,
    Android,
    Ios,
    Other,
}

impl OsFamily {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::Macos,
            "linux" => Self::Linux,
            "android" => Self::Android,
            "ios" => Self::Ios,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct OsInfo {
    pub family: OsFamily,
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CpuInfo {
    pub brand: String,
    pub vendor: String,
    pub physical_cores: Option<u32>,
    pub logical_cores: u32,
    /// Reported base/current frequency in MHz.
    pub frequency_mhz: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Qualcomm,
    Arm,
    Microsoft,
    Other,
}

impl GpuVendor {
    /// Map a PCI vendor id to a vendor.
    pub fn from_pci_id(id: u32) -> Self {
        match id {
            0x10de => Self::Nvidia,
            0x1002 | 0x1022 => Self::Amd,
            0x8086 => Self::Intel,
            0x106b => Self::Apple,
            0x5143 | 0x17cb => Self::Qualcomm,
            0x13b5 => Self::Arm,
            0x1414 => Self::Microsoft,
            _ => Self::Other,
        }
    }

    /// Best-effort vendor detection from a marketing name.
    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("nvidia")
            || n.contains("geforce")
            || n.contains("rtx")
            || n.contains("quadro")
        {
            Self::Nvidia
        } else if n.contains("amd") || n.contains("radeon") {
            Self::Amd
        } else if n.contains("intel") || n.contains("iris") {
            Self::Intel
        } else if n.contains("apple") {
            Self::Apple
        } else if n.contains("adreno") || n.contains("qualcomm") {
            Self::Qualcomm
        } else if n.contains("mali") {
            Self::Arm
        } else if n.contains("microsoft") {
            Self::Microsoft
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    /// Dedicated video memory. `None` if unknown; integrated GPUs typically report a small value or 0.
    pub dedicated_vram_bytes: Option<u64>,
    /// System memory the GPU can borrow (integrated GPUs, unified memory).
    pub shared_memory_bytes: Option<u64>,
    pub driver_version: Option<String>,
    pub integrated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NpuInfo {
    pub name: String,
    pub vendor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct BatteryInfo {
    /// 0.0 - 100.0
    pub charge_percent: f64,
    pub design_capacity_wh: Option<f64>,
    pub full_capacity_wh: Option<f64>,
    pub charging: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}
