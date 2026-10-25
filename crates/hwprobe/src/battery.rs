use infermark_sdk::{BatteryInfo, PowerSource};
use starship_battery::units::energy::watt_hour;
use starship_battery::State;

pub fn detect() -> (Option<BatteryInfo>, PowerSource) {
    let battery = first_battery();
    let power = platform_power_source().unwrap_or(match &battery {
        None => PowerSource::Ac,
        Some(b) if b.charging => PowerSource::Ac,
        Some(_) => PowerSource::Unknown,
    });
    (battery, power)
}

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
