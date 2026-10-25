//! Fill in NVIDIA details via NVML when the driver is installed (loaded at runtime, so builds
//! without NVIDIA hardware are unaffected).

use infermark_sdk::{GpuInfo, GpuVendor};
use nvml_wrapper::Nvml;

pub fn enrich(gpus: &mut Vec<GpuInfo>) {
    let Ok(nvml) = Nvml::init() else { return };
    let driver = nvml.sys_driver_version().ok();
    let Ok(count) = nvml.device_count() else {
        return;
    };

    // NVIDIA entries the platform already found, matched to NVML devices in order.
    let mut slots = gpus
        .iter()
        .enumerate()
        .filter(|(_, g)| g.vendor == GpuVendor::Nvidia)
        .map(|(i, _)| i)
        .collect::<Vec<_>>()
        .into_iter();

    for index in 0..count {
        let Ok(device) = nvml.device_by_index(index) else {
            continue;
        };
        let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".into());
        let vram = device.memory_info().ok().map(|m| m.total);

        match slots.next() {
            Some(i) => {
                let g = &mut gpus[i];
                if driver.is_some() {
                    g.driver_version = driver.clone();
                }
                if vram.is_some() {
                    g.dedicated_vram_bytes = vram;
                }
            }
            None => gpus.push(GpuInfo {
                name,
                vendor: GpuVendor::Nvidia,
                dedicated_vram_bytes: vram,
                shared_memory_bytes: None,
                driver_version: driver.clone(),
                integrated: false,
            }),
        }
    }
}
