//! Battery and power. On Android the app reads these through Android's own APIs (a Tauri
//! plugin) and fills them in, so the readers here report nothing there.

use infermark_sdk::{BatteryInfo, PowerSource};
#[cfg(not(target_os = "android"))]
use starship_battery::units::energy::watt_hour;
#[cfg(not(target_os = "android"))]
use starship_battery::units::power::watt;
#[cfg(not(target_os = "android"))]
use starship_battery::units::thermodynamic_temperature::degree_celsius;
#[cfg(not(target_os = "android"))]
use starship_battery::State;

/// One reading of the first battery, for telemetry. Unrounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryReading {
    pub charge_percent: f64,
    /// Watts drawn from the battery; `None` while charging, full, or not reported.
    pub discharge_w: Option<f64>,
    /// Most laptops don't report this; `None` then.
    pub temperature_c: Option<f64>,
}

/// `None` when this device has no battery or it can't be read right now.
#[cfg(target_os = "android")]
pub fn read_battery() -> Option<BatteryReading> {
    None
}

/// `None` when this device has no battery or it can't be read right now.
#[cfg(not(target_os = "android"))]
pub fn read_battery() -> Option<BatteryReading> {
    let manager = starship_battery::Manager::new().ok()?;
    let b = manager.batteries().ok()?.flatten().next()?;
    let discharging = matches!(b.state(), State::Discharging);
    Some(BatteryReading {
        charge_percent: f64::from(b.state_of_charge().value) * 100.0,
        discharge_w: discharging
            .then(|| f64::from(b.energy_rate().get::<watt>()))
            .filter(|w| w.is_finite() && *w > 0.0),
        temperature_c: b
            .temperature()
            .map(|t| f64::from(t.get::<degree_celsius>()))
            .filter(|t| t.is_finite()),
    })
}

pub fn detect() -> (Option<BatteryInfo>, PowerSource) {
    let battery = first_battery();
    let power = platform_power_source().unwrap_or(match &battery {
        None => PowerSource::Ac,
        Some(b) if b.charging => PowerSource::Ac,
        Some(_) => PowerSource::Unknown,
    });
    (battery, power)
}

#[cfg(target_os = "android")]
fn first_battery() -> Option<BatteryInfo> {
    None
}

#[cfg(not(target_os = "android"))]
fn first_battery() -> Option<BatteryInfo> {
    let manager = starship_battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.flatten().next()?;
    let charge = f64::from(battery.state_of_charge().value) * 100.0;
    Some(BatteryInfo {
        charge_percent: (charge * 10.0).round() / 10.0,
        design_capacity_wh: positive(f64::from(battery.energy_full_design().get::<watt_hour>())),
        full_capacity_wh: positive(f64::from(battery.energy_full().get::<watt_hour>())),
        charging: matches!(battery.state(), State::Charging | State::Full),
    })
}

#[cfg(not(target_os = "android"))]
fn positive(v: f64) -> Option<f64> {
    (v > 0.0).then_some((v * 10.0).round() / 10.0)
}

#[cfg(windows)]
fn platform_power_source() -> Option<PowerSource> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: GetSystemPowerStatus only writes into the struct we pass.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    match status.ACLineStatus {
        0 => Some(PowerSource::Battery),
        1 => Some(PowerSource::Ac),
        _ => None,
    }
}

#[cfg(not(windows))]
fn platform_power_source() -> Option<PowerSource> {
    None
}

/// Whether the OS's battery saver / low power mode is on; `None` when it can't be told.
pub fn power_saver() -> Option<bool> {
    platform_power_saver()
}

#[cfg(windows)]
fn platform_power_saver() -> Option<bool> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: GetSystemPowerStatus only writes into the struct we pass.
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    Some(status.SystemStatusFlag == 1)
}

/// The ACPI platform profile, which desktop power settings switch to `low-power`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn platform_power_saver() -> Option<bool> {
    let profile = std::fs::read_to_string("/sys/firmware/acpi/platform_profile").ok()?;
    Some(profile.trim() == "low-power")
}

/// `pmset -g` lists `lowpowermode 1` while Low Power Mode is on.
#[cfg(target_os = "macos")]
fn platform_power_saver() -> Option<bool> {
    let out = std::process::Command::new("pmset")
        .arg("-g")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("lowpowermode"))?;
    Some(line.split_whitespace().nth(1) == Some("1"))
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos"
)))]
fn platform_power_saver() -> Option<bool> {
    None
}
