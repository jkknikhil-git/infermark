//! Hardware detection. [`probe`] never fails: anything that can't be detected is left empty or
//! `None` so results show it as missing rather than wrong.

mod battery;
mod gpu;
mod nvidia;

use infermark_sdk::{CpuInfo, HardwareInfo, MemoryInfo, OsFamily, OsInfo};
use sha2::{Digest, Sha256};
use sysinfo::System;

/// Detect the current machine.
pub fn probe() -> HardwareInfo {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu = cpu_info(&sys);
    let memory = MemoryInfo {
        total_bytes: sys.total_memory(),
        available_bytes: sys.available_memory(),
    };
    let os = OsInfo {
        family: OsFamily::current(),
        name: System::name().unwrap_or_else(|| std::env::consts::OS.to_string()),
        version: System::os_version().unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.to_string(),
    };

    let (mut gpus, npus) = gpu::detect(memory.total_bytes);
    nvidia::enrich(&mut gpus);
    let (battery, power_source) = battery::detect();

    let mut hw = HardwareInfo {
        fingerprint: String::new(),
        os,
        cpu,
        memory,
        gpus,
        npus,
        battery,
        power_source,
    };
    hw.fingerprint = fingerprint(&hw);
    hw
}

fn cpu_info(sys: &System) -> CpuInfo {
    let first = sys.cpus().first();
    CpuInfo {
        brand: first
            .map(|c| c.brand().trim().to_string())
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "Unknown CPU".into()),
        vendor: first.map(|c| c.vendor_id().to_string()).unwrap_or_default(),
        physical_cores: sys.physical_core_count().map(|c| c as u32),
        logical_cores: sys.cpus().len() as u32,
        frequency_mhz: first.map(|c| c.frequency()).filter(|f| *f > 0),
    }
}

/// Anonymous, stable id for "this hardware configuration". Free memory, battery level and
/// driver versions are excluded so the fingerprint doesn't change between runs.
pub fn fingerprint(hw: &HardwareInfo) -> String {
    let mut gpu_names: Vec<&str> = hw.gpus.iter().map(|g| g.name.as_str()).collect();
    gpu_names.sort_unstable();
    let ram_gb = (hw.total_ram_gb()).round() as u64;
    let key = format!(
        "{}|{}|{}|{}|{:?}|{}",
        hw.cpu.brand,
        hw.cpu.logical_cores,
        ram_gb,
        gpu_names.join(","),
        hw.os.family,
        hw.os.arch,
    );
    let digest = Sha256::digest(key.as_bytes());
    hex::encode(&digest[..8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_detects_basics() {
        let hw = probe();
        assert!(hw.cpu.logical_cores >= 1);
        assert!(hw.memory.total_bytes > 0);
        assert_eq!(hw.fingerprint.len(), 16);
        assert_eq!(hw.os.family, OsFamily::current());
    }

    #[test]
    fn fingerprint_ignores_volatile_fields() {
        let mut a = probe();
        let mut b = a.clone();
        b.memory.available_bytes = a.memory.available_bytes / 2;
        for g in &mut b.gpus {
            g.driver_version = Some("changed".into());
        }
        assert_eq!(fingerprint(&a), fingerprint(&b));
        a.cpu.brand.push_str(" X");
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }
}
