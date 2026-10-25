//! Per-platform GPU and NPU detection.

use infermark_sdk::{GpuInfo, NpuInfo};

/// Below this much dedicated memory a GPU is treated as integrated (it borrows system RAM).
const INTEGRATED_VRAM_LIMIT: u64 = 1024 * 1024 * 1024;

pub fn detect(total_ram: u64) -> (Vec<GpuInfo>, Vec<NpuInfo>) {
    platform::detect(total_ram)
}

#[cfg(windows)]
mod platform {
    use std::collections::HashMap;

    use infermark_sdk::{GpuInfo, GpuVendor, NpuInfo};
    use serde::Deserialize;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE,
    };

    use super::INTEGRATED_VRAM_LIMIT;

    pub fn detect(_total_ram: u64) -> (Vec<GpuInfo>, Vec<NpuInfo>) {
        // WMI needs its own COM apartment; a fresh thread avoids clashing with whatever the
        // caller (e.g. the desktop webview) already initialised.
        let wmi = std::thread::spawn(wmi_info).join().unwrap_or_default();
        let gpus = dxgi_adapters()
            .into_iter()
            .map(|mut g| {
                g.driver_version = wmi.drivers.get(&g.name).cloned();
                g
            })
            .collect();
        (gpus, wmi.npus)
    }

    fn dxgi_adapters() -> Vec<GpuInfo> {
        let mut out = Vec::new();
        // SAFETY: plain DXGI enumeration; every returned interface is reference counted.
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
                return out;
            };
            let mut index = 0;
            while let Ok(adapter) = factory.EnumAdapters1(index) {
                index += 1;
                let Ok(desc) = adapter.GetDesc1() else {
                    continue;
                };
                if desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
                    continue;
                }
                let name = String::from_utf16_lossy(&desc.Description)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                let dedicated = desc.DedicatedVideoMemory as u64;
                let vendor = match GpuVendor::from_pci_id(desc.VendorId) {
                    GpuVendor::Other => GpuVendor::from_name(&name),
                    v => v,
                };
                if vendor == GpuVendor::Microsoft {
                    continue; // Basic Render Driver and remote display adapters
                }
                // The same adapter can be listed more than once (one per output).
                if out
                    .iter()
                    .any(|g: &GpuInfo| g.name == name && g.dedicated_vram_bytes == Some(dedicated))
                {
                    continue;
                }
                out.push(GpuInfo {
                    name,
                    vendor,
                    dedicated_vram_bytes: Some(dedicated),
                    shared_memory_bytes: Some(desc.SharedSystemMemory as u64),
                    driver_version: None,
                    integrated: dedicated < INTEGRATED_VRAM_LIMIT,
                });
            }
        }
        out
    }

    #[derive(Default)]
    struct WmiInfo {
        drivers: HashMap<String, String>,
        npus: Vec<NpuInfo>,
    }

    #[derive(Deserialize)]
    #[serde(rename = "Win32_VideoController", rename_all = "PascalCase")]
    struct VideoController {
        name: Option<String>,
        driver_version: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct PnpEntity {
        name: Option<String>,
        manufacturer: Option<String>,
    }

    fn wmi_info() -> WmiInfo {
        let mut info = WmiInfo::default();
        let Ok(com) = wmi::COMLibrary::new() else {
            return info;
        };
        let Ok(conn) = wmi::WMIConnection::new(com) else {
            return info;
        };

        if let Ok(rows) = conn.query::<VideoController>() {
            for row in rows {
                if let (Some(name), Some(version)) = (row.name, row.driver_version) {
                    info.drivers.insert(name.trim().to_string(), version);
                }
            }
        }
        // NPUs (Intel AI Boost, AMD Ryzen AI, Qualcomm Hexagon) register as compute accelerators.
        if let Ok(rows) = conn.raw_query::<PnpEntity>(
            "SELECT Name, Manufacturer FROM Win32_PnPEntity WHERE PNPClass = 'ComputeAccelerator'",
        ) {
            info.npus = rows
                .into_iter()
                .filter_map(|r| {
                    Some(NpuInfo {
                        name: r.name?.trim().to_string(),
                        vendor: r.manufacturer.unwrap_or_default(),
                    })
                })
                .collect();
        }
        info
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::path::Path;

    use infermark_sdk::{GpuInfo, GpuVendor, NpuInfo};

    use super::INTEGRATED_VRAM_LIMIT;

    pub fn detect(_total_ram: u64) -> (Vec<GpuInfo>, Vec<NpuInfo>) {
        let mut gpus = Vec::new();
        let Ok(entries) = fs::read_dir("/sys/class/drm") else {
            return (gpus, npus());
        };
        let mut cards: Vec<_> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("card") && !n.contains('-'))
            .collect();
        cards.sort();
        for card in cards {
            let dev = Path::new("/sys/class/drm").join(&card).join("device");
            let Some(vendor_id) = read_hex(&dev.join("vendor")) else {
                continue;
            };
            let device_id = read_hex(&dev.join("device")).unwrap_or(0);
            let vendor = GpuVendor::from_pci_id(vendor_id);
            let vram = read_u64(&dev.join("mem_info_vram_total"));
            let driver = fs::read_link(dev.join("driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
            gpus.push(GpuInfo {
                name: format!("{vendor:?} GPU [{vendor_id:04x}:{device_id:04x}]"),
                vendor,
                dedicated_vram_bytes: vram,
                shared_memory_bytes: None,
                driver_version: driver,
                integrated: vram.map_or(vendor == GpuVendor::Intel, |v| v < INTEGRATED_VRAM_LIMIT),
            });
        }
        (gpus, npus())
    }

    fn npus() -> Vec<NpuInfo> {
        // Intel NPU (intel_vpu) and AMD XDNA expose accel devices.
        let Ok(entries) = fs::read_dir("/sys/class/accel") else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| {
                let driver = fs::read_link(e.path().join("device/driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_default();
                NpuInfo {
                    name: format!("NPU ({driver})"),
                    vendor: driver,
                }
            })
            .collect()
    }

    fn read_hex(path: &Path) -> Option<u32> {
        let s = fs::read_to_string(path).ok()?;
        u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
    }

    fn read_u64(path: &Path) -> Option<u64> {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    use infermark_sdk::{GpuInfo, GpuVendor, NpuInfo};

    pub fn detect(total_ram: u64) -> (Vec<GpuInfo>, Vec<NpuInfo>) {
        let mut gpus = Vec::new();
        let mut npus = Vec::new();
        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType", "-json"])
            .output();
        let Ok(output) = output else {
            return (gpus, npus);
        };
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
            return (gpus, npus);
        };
        for item in json["SPDisplaysDataType"].as_array().into_iter().flatten() {
            let name = item["sppci_model"].as_str().unwrap_or("GPU").to_string();
            let vendor = GpuVendor::from_name(&name);
            let apple = vendor == GpuVendor::Apple;
            gpus.push(GpuInfo {
                vendor,
                dedicated_vram_bytes: item["spdisplays_vram"].as_str().and_then(parse_size),
                // Apple Silicon shares unified memory between CPU and GPU.
                shared_memory_bytes: apple.then_some(total_ram),
                driver_version: None,
                integrated: apple,
                name,
            });
            if apple {
                npus.push(NpuInfo {
                    name: "Apple Neural Engine".into(),
                    vendor: "Apple".into(),
                });
            }
        }
        npus.truncate(1);
        (gpus, npus)
    }

    fn parse_size(s: &str) -> Option<u64> {
        let mut parts = s.split_whitespace();
        let n: u64 = parts.next()?.parse().ok()?;
        match parts.next()? {
            "GB" => Some(n * 1024 * 1024 * 1024),
            "MB" => Some(n * 1024 * 1024),
            _ => None,
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use infermark_sdk::{GpuInfo, NpuInfo};

    pub fn detect(_total_ram: u64) -> (Vec<GpuInfo>, Vec<NpuInfo>) {
        (Vec::new(), Vec::new())
    }
}
