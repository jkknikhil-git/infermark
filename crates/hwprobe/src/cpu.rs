//! CPU core layout: which cores are fast and which are efficient.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use infermark_sdk::CpuCluster;

/// Clusters of identical cores, fastest first; empty when the layout can't be read.
pub fn clusters() -> Vec<CpuCluster> {
    detect()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn detect() -> Vec<CpuCluster> {
    from_sysfs(Path::new("/sys/devices/system/cpu"))
}

/// Logical CPUs grouped by their highest clock (`cpufreq/cpuinfo_max_freq`), with physical
/// cores counted from `topology/core_id`. Phones expose this even to normal apps.
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
fn from_sysfs(root: &Path) -> Vec<CpuCluster> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    // max kHz -> (physical cores, logical count)
    let mut groups: BTreeMap<u32, (BTreeSet<(String, String)>, u32)> = BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_cpu = name
            .strip_prefix("cpu")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if !is_cpu {
            continue;
        }
        let dir = entry.path();
        let read = |rel: &str| {
            std::fs::read_to_string(dir.join(rel))
                .ok()
                .map(|s| s.trim().to_string())
        };
        // Offline CPUs and CPUs without cpufreq: without every core's clock the layout
        // would be wrong, so report nothing rather than half a picture.
        let Some(khz) = read("cpufreq/cpuinfo_max_freq").and_then(|s| s.parse::<u32>().ok()) else {
            return Vec::new();
        };
        let core = (
            read("topology/physical_package_id").unwrap_or_default(),
            read("topology/core_id").unwrap_or_else(|| name.clone()),
        );
        let g = groups.entry(khz).or_default();
        g.0.insert(core);
        g.1 += 1;
    }
    groups
        .into_iter()
        .rev()
        .map(|(khz, (cores, logical))| CpuCluster {
            physical_cores: cores.len() as u32,
            logical_cores: logical,
            max_mhz: Some(khz / 1000),
        })
        .collect()
}

/// Cores grouped by Windows' efficiency class (higher is faster). Clocks per class aren't
/// available without a driver, so `max_mhz` is unknown.
#[cfg(windows)]
fn detect() -> Vec<CpuCluster> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    let mut len = 0u32;
    // SAFETY: with no buffer the call only writes the required size into `len`.
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len) };
    if len == 0 {
        return Vec::new();
    }
    // u64 storage keeps the records 8-byte aligned. Records are shorter than the struct (its
    // union is sized for the largest kind), so zeroed padding of one struct keeps every
    // reference inside the allocation.
    let pad = std::mem::size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>();
    let mut buf = vec![0u64; (len as usize + pad).div_ceil(8)];
    // SAFETY: `buf` holds at least `len` bytes, as the first call asked for.
    let ok = unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(
                buf.as_mut_ptr()
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>(),
            ),
            &mut len,
        )
    };
    if ok.is_err() {
        return Vec::new();
    }

    // efficiency class -> (physical, logical)
    let mut classes: BTreeMap<u8, (u32, u32)> = BTreeMap::new();
    let base = buf.as_ptr().cast::<u8>();
    let mut offset = 0usize;
    // Every record starts with its kind and size (8 bytes).
    while offset + 8 <= len as usize {
        // SAFETY: records are laid out back to back within the `len` bytes Windows filled in,
        // each starting on an aligned boundary, and `Size` gives the length of each.
        let info = unsafe {
            &*base
                .add(offset)
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        if info.Relationship == RelationProcessorCore {
            // SAFETY: for RelationProcessorCore the union holds a PROCESSOR_RELATIONSHIP whose
            // GroupMask array has GroupCount entries.
            let core = unsafe { &info.Anonymous.Processor };
            let masks = unsafe {
                std::slice::from_raw_parts(core.GroupMask.as_ptr(), core.GroupCount as usize)
            };
            let logical: u32 = masks.iter().map(|m| m.Mask.count_ones()).sum();
            let c = classes.entry(core.EfficiencyClass).or_default();
            c.0 += 1;
            c.1 += logical;
        }
        if info.Size == 0 {
            break;
        }
        offset += info.Size as usize;
    }
    classes
        .into_values()
        .rev()
        .map(|(physical, logical)| CpuCluster {
            physical_cores: physical,
            logical_cores: logical,
            max_mhz: None,
        })
        .collect()
}

/// Apple silicon performance levels (`hw.perflevel0` is the fastest).
#[cfg(target_os = "macos")]
fn detect() -> Vec<CpuCluster> {
    let levels = sysctl_u32("hw.nperflevels").unwrap_or(0);
    (0..levels)
        .filter_map(|i| {
            Some(CpuCluster {
                physical_cores: sysctl_u32(&format!("hw.perflevel{i}.physicalcpu"))?,
                logical_cores: sysctl_u32(&format!("hw.perflevel{i}.logicalcpu"))?,
                max_mhz: None,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn sysctl_u32(name: &str) -> Option<u32> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>();
    // SAFETY: `value` is a u32 and `size` says so; sysctlbyname writes at most `size` bytes.
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0 && size == std::mem::size_of::<u32>()).then_some(value)
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    windows,
    target_os = "macos"
)))]
fn detect() -> Vec<CpuCluster> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(root: &Path, n: u32, khz: Option<u32>, core: u32) {
        let d = root.join(format!("cpu{n}"));
        std::fs::create_dir_all(d.join("topology")).unwrap();
        std::fs::write(d.join("topology/core_id"), format!("{core}\n")).unwrap();
        std::fs::write(d.join("topology/physical_package_id"), "0\n").unwrap();
        if let Some(k) = khz {
            std::fs::create_dir_all(d.join("cpufreq")).unwrap();
            std::fs::write(d.join("cpufreq/cpuinfo_max_freq"), format!("{k}\n")).unwrap();
        }
    }

    #[test]
    fn phone_layout_prime_big_little() {
        let dir = tempfile::tempdir().unwrap();
        // Snapdragon-style 1 + 3 + 4.
        cpu(dir.path(), 0, Some(1_800_000), 0);
        cpu(dir.path(), 1, Some(1_800_000), 1);
        cpu(dir.path(), 2, Some(1_800_000), 2);
        cpu(dir.path(), 3, Some(1_800_000), 3);
        cpu(dir.path(), 4, Some(2_800_000), 4);
        cpu(dir.path(), 5, Some(2_800_000), 5);
        cpu(dir.path(), 6, Some(2_800_000), 6);
        cpu(dir.path(), 7, Some(3_300_000), 7);
        std::fs::create_dir_all(dir.path().join("cpufreq")).unwrap(); // not a CPU
        let c = from_sysfs(dir.path());
        let shape: Vec<_> = c
            .iter()
            .map(|c| (c.physical_cores, c.logical_cores, c.max_mhz))
            .collect();
        assert_eq!(
            shape,
            [(1, 1, Some(3300)), (3, 3, Some(2800)), (4, 4, Some(1800))]
        );
    }

    #[test]
    fn hyperthreads_count_once_as_physical_cores() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..8 {
            cpu(dir.path(), n, Some(4_500_000), n % 4);
        }
        let c = from_sysfs(dir.path());
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].physical_cores, c[0].logical_cores), (4, 8));
    }

    #[test]
    fn missing_clock_means_unknown_layout() {
        let dir = tempfile::tempdir().unwrap();
        cpu(dir.path(), 0, Some(2_000_000), 0);
        cpu(dir.path(), 1, None, 1);
        assert!(from_sysfs(dir.path()).is_empty());
        assert!(from_sysfs(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn this_machine_layout_adds_up() {
        let c = clusters();
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        if !c.is_empty() {
            let logical: u32 = c.iter().map(|c| c.logical_cores).sum();
            assert_eq!(logical as usize, sys.cpus().len(), "{c:?}");
            assert!(c
                .iter()
                .all(|c| c.physical_cores >= 1 && c.logical_cores >= c.physical_cores));
        }
    }
}
