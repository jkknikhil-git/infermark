//! Turning what Android reports into InferMark's types. Pure functions, compiled on every
//! platform so they're tested on desktop CI too; `android.rs` feeds them real readings.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use infermark_sdk::{BatteryInfo, HardwareInfo, PowerSource};
use serde::Deserialize;

/// One reading of everything the plugin reports. Fields are `None` when Android doesn't say.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snapshot {
    pub battery_percent: Option<f64>,
    /// Plugged into a charger (any kind).
    pub plugged: bool,
    pub discharging: bool,
    /// Battery current; the sign and even the unit (µA or mA) vary by phone maker.
    pub current: Option<f64>,
    pub voltage_mv: Option<f64>,
    pub battery_temp_c: Option<f64>,
    /// `PowerManager.getCurrentThermalStatus()`: 0 none … 6 shutdown.
    pub thermal_status: Option<i32>,
    pub power_save: Option<bool>,
    pub manufacturer: String,
    pub model: String,
    pub soc_manufacturer: Option<String>,
    pub soc_model: Option<String>,
}

/// Fill in what the Rust probe can't see on Android: the battery, the power source and the
/// chip's name (sysinfo has no CPU brand on ARM).
pub fn apply(hw: &mut HardwareInfo, s: &Snapshot) {
    if let Some(battery) = battery_info(s) {
        hw.power_source = power_source(s);
        hw.battery = Some(battery);
    }
    if let Some(chip) = chip_name(s) {
        if hw.cpu.brand.is_empty() || hw.cpu.brand == "Unknown CPU" {
            hw.cpu.brand = chip;
        }
    }
    hw.fingerprint = infermark_hwprobe::fingerprint(hw);
}

pub fn battery_info(s: &Snapshot) -> Option<BatteryInfo> {
    s.battery_percent.map(|p| BatteryInfo {
        charge_percent: (p * 10.0).round() / 10.0,
        design_capacity_wh: None,
        full_capacity_wh: None,
        charging: s.plugged,
    })
}

pub fn power_source(s: &Snapshot) -> PowerSource {
    if s.plugged {
        PowerSource::Ac
    } else {
        PowerSource::Battery
    }
}

/// "Qualcomm SM8550" from the SoC fields (Android 12+), else the phone's model name.
fn chip_name(s: &Snapshot) -> Option<String> {
    let soc = [s.soc_manufacturer.as_deref(), s.soc_model.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("unknown"))
        .collect::<Vec<_>>()
        .join(" ");
    if !soc.is_empty() {
        return Some(soc);
    }
    let phone = format!("{} {}", s.manufacturer.trim(), s.model.trim());
    (!phone.trim().is_empty()).then(|| phone.trim().to_string())
}

/// Watts drawn from the battery while discharging. Phones report current in µA, but some
/// makers use mA and the sign varies, so the size decides: a phone running a model draws
/// hundreds of mA, which is over 20,000 when counted in µA.
pub fn drain_watts(s: &Snapshot) -> Option<f64> {
    if !s.discharging {
        return None;
    }
    let i = s.current?.abs();
    let v = s.voltage_mv?;
    if i == 0.0 || v <= 0.0 {
        return None;
    }
    let amps = if i < 20_000.0 { i / 1e3 } else { i / 1e6 };
    Some(amps * v / 1e3)
}

/// Android's thermal status (none, light, moderate, severe, critical, emergency, shutdown) on
/// the shared 0-3 scale: nominal, fair, serious, critical.
pub fn thermal_level(status: i32) -> Option<f64> {
    match status {
        0 => Some(0.0),
        1 => Some(1.0),
        2 => Some(2.0),
        3..=6 => Some(3.0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            battery_percent: Some(76.0),
            discharging: true,
            current: Some(-850_000.0),
            voltage_mv: Some(3_900.0),
            manufacturer: "Google".into(),
            model: "Pixel 8".into(),
            soc_manufacturer: Some("Google".into()),
            soc_model: Some("Tensor G3".into()),
            ..Default::default()
        }
    }

    #[test]
    fn drain_handles_microamps_milliamps_and_charging() {
        // 850 mA at 3.9 V, reported in µA with a negative sign.
        let w = drain_watts(&snap()).unwrap();
        assert!((w - 3.315).abs() < 1e-9, "{w}");
        // The same current reported in mA.
        let ma = Snapshot {
            current: Some(850.0),
            ..snap()
        };
        assert!((drain_watts(&ma).unwrap() - 3.315).abs() < 1e-9);
        let charging = Snapshot {
            discharging: false,
            ..snap()
        };
        assert_eq!(drain_watts(&charging), None);
        let unknown = Snapshot {
            current: None,
            ..snap()
        };
        assert_eq!(drain_watts(&unknown), None);
    }

    #[test]
    fn thermal_status_maps_to_four_levels() {
        let levels: Vec<_> = (0..=6).map(thermal_level).collect();
        assert_eq!(
            levels,
            [
                Some(0.0),
                Some(1.0),
                Some(2.0),
                Some(3.0),
                Some(3.0),
                Some(3.0),
                Some(3.0)
            ]
        );
        assert_eq!(thermal_level(-1), None);
    }

    #[test]
    fn chip_name_prefers_the_soc_then_the_phone() {
        assert_eq!(chip_name(&snap()).as_deref(), Some("Google Tensor G3"));
        let old = Snapshot {
            soc_manufacturer: None,
            soc_model: Some("unknown".into()),
            ..snap()
        };
        assert_eq!(chip_name(&old).as_deref(), Some("Google Pixel 8"));
    }

    #[test]
    fn apply_fills_battery_power_and_brand() {
        let mut hw: HardwareInfo = serde_json::from_value(serde_json::json!({
            "fingerprint": "", "os": {"family": "android", "name": "Android", "version": "14", "kernel": "", "arch": "aarch64"},
            "cpu": {"brand": "Unknown CPU", "vendor": "", "physical_cores": 8, "logical_cores": 8, "frequency_mhz": null},
            "memory": {"total_bytes": 8589934592u64, "available_bytes": 3221225472u64},
            "gpus": [], "npus": [], "battery": null, "power_source": "unknown"
        }))
        .unwrap();
        apply(&mut hw, &snap());
        assert_eq!(hw.cpu.brand, "Google Tensor G3");
        assert_eq!(hw.power_source, PowerSource::Battery);
        assert_eq!(hw.battery.as_ref().unwrap().charge_percent, 76.0);
        assert!(!hw.fingerprint.is_empty());
    }
}
