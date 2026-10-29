//! Android: device details and sensors from Android's own APIs (BatteryManager, PowerManager,
//! `Build`), through the Kotlin plugin `dev.infermark.device.DevicePlugin` in `gen/android`.
//! Apps can't read most of `/sys` and `/proc` on Android, so this is the only reliable source.

use infermark_sdk::{SensorChannel, SensorKind, SensorProvider, SensorReading};
use serde::Serialize;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Manager, Wry};

use crate::phone::{drain_watts, thermal_level, Snapshot};

/// Handle to the Kotlin plugin, kept in Tauri's state.
#[derive(Clone)]
pub struct Device(PluginHandle<Wry>);

#[derive(Serialize)]
struct KeepScreenOn {
    on: bool,
}

impl Device {
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.0.run_mobile_plugin("snapshot", ()).ok()
    }

    /// Keep the screen on while a run is going (Android dims and throttles otherwise).
    pub fn keep_screen_on(&self, on: bool) {
        let _ = self
            .0
            .run_mobile_plugin::<serde_json::Value>("keepScreenOn", KeepScreenOn { on });
    }
}

pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new("device")
        .setup(|app, api| {
            let handle = api.register_android_plugin("dev.infermark.device", "DevicePlugin")?;
            app.manage(Device(handle));
            Ok(())
        })
        .build()
}

pub fn device(app: &tauri::AppHandle) -> Option<Device> {
    app.try_state::<Device>().map(|d| d.inner().clone())
}

/// Battery and thermal readings for the run's telemetry.
pub struct AndroidProvider {
    device: Device,
    has_temperature: bool,
    has_thermal: bool,
}

impl AndroidProvider {
    pub fn new(device: Device) -> Self {
        let first = device.snapshot().unwrap_or_default();
        Self {
            has_temperature: first.battery_temp_c.is_some(),
            has_thermal: first.thermal_status.is_some(),
            device,
        }
    }
}

impl SensorProvider for AndroidProvider {
    fn id(&self) -> &'static str {
        "android"
    }

    fn channels(&self) -> Vec<SensorChannel> {
        let mut c = vec![
            SensorChannel::new("battery", SensorKind::BatteryCharge, "Battery charge"),
            SensorChannel::new("battery", SensorKind::Power, "Battery drain"),
        ];
        if self.has_temperature {
            c.push(SensorChannel::new(
                "battery",
                SensorKind::Temperature,
                "Battery temperature",
            ));
        }
        if self.has_thermal {
            c.push(SensorChannel::new(
                "device",
                SensorKind::ThermalState,
                "Thermal state",
            ));
        }
        c
    }

    fn sample(&mut self) -> Vec<SensorReading> {
        let s = self.device.snapshot().unwrap_or_default();
        let mut out = vec![
            SensorReading::new("battery", SensorKind::BatteryCharge, s.battery_percent),
            SensorReading::new("battery", SensorKind::Power, drain_watts(&s)),
        ];
        if self.has_temperature {
            out.push(SensorReading::new(
                "battery",
                SensorKind::Temperature,
                s.battery_temp_c,
            ));
        }
        if self.has_thermal {
            out.push(SensorReading::new(
                "device",
                SensorKind::ThermalState,
                s.thermal_status.and_then(thermal_level),
            ));
        }
        out
    }
}
