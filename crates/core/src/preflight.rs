//! Checks made just before a run: anything that would make results lower than this device can
//! really do. They're shown before Start, sent at the start of the run and saved in the result,
//! so a slow number can be read in context later.

use infermark_sdk::{
    BatteryInfo, CheckLevel, MemoryInfo, PowerSource, PreflightCheck, Profile, SensorKind,
    SensorReading,
};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

/// What the checks look at. Built from the hardware probe and one round of sensor readings.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub memory: MemoryInfo,
    pub battery: Option<BatteryInfo>,
    pub power_source: PowerSource,
    /// Battery saver / low power mode; `None` when the OS doesn't say.
    pub power_saver: Option<bool>,
    /// Latest sensor readings (CPU load, temperatures, thermal state).
    pub readings: Vec<SensorReading>,
}

impl Inputs {
    /// The current state of this device: memory, power and one CPU-load reading. Takes about
    /// a third of a second, because CPU load needs two samples.
    pub fn current() -> Self {
        let live = infermark_hwprobe::live_status();
        let mut sensors = infermark_telemetry::default_providers();
        Self {
            memory: live.memory,
            battery: live.battery,
            power_source: live.power_source,
            power_saver: infermark_hwprobe::power_saver(),
            readings: sample_twice(&mut sensors),
        }
    }
}

/// Two rounds of sensor readings a moment apart (CPU load is only known from the second).
pub fn sample_twice(sensors: &mut [Box<dyn infermark_sdk::SensorProvider>]) -> Vec<SensorReading> {
    if sensors.is_empty() {
        return Vec::new();
    }
    for s in sensors.iter_mut() {
        s.sample();
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    sensors.iter_mut().flat_map(|s| s.sample()).collect()
}

/// Everything worth saying before `profile` runs, most important first.
pub fn check(profile: &Profile, inputs: &Inputs) -> Vec<PreflightCheck> {
    let mut out = Vec::new();
    let mut add = |id: &str, level: CheckLevel, message: String| {
        out.push(PreflightCheck {
            id: id.into(),
            level,
            message,
        })
    };
    let battery_test = profile
        .modules
        .iter()
        .any(|(id, m)| id == "battery-drain" && m.enabled);
    let on_battery = inputs.power_source == PowerSource::Battery;

    let total = inputs.memory.total_bytes as f64;
    let free = inputs.memory.available_bytes as f64;
    if total > 0.0 && (free / total < 0.25 || free < 2.0 * GIB) {
        add(
            "low-memory",
            CheckLevel::Warning,
            format!(
                "Only {:.1} GB of {:.1} GB memory is free. Close other apps first, or speed can \
                 come out well below what this device can do.",
                free / GIB,
                total / GIB
            ),
        );
    }

    let value = |family: &str, kind: SensorKind| {
        inputs
            .readings
            .iter()
            .filter(|r| {
                r.kind == kind && r.source.trim_end_matches(|c: char| c.is_ascii_digit()) == family
            })
            .find_map(|r| r.value)
    };
    if let Some(load) = value("cpu", SensorKind::Utilization).filter(|l| *l >= 25.0) {
        add(
            "busy-cpu",
            CheckLevel::Warning,
            format!(
                "Other apps are using {load:.0}% of the CPU right now. Close them or wait for \
                 them to finish."
            ),
        );
    }

    if inputs.power_saver == Some(true) {
        add(
            "battery-saver",
            CheckLevel::Warning,
            "Battery saver is on, which slows the CPU and GPU. Turn it off for full-speed \
             results."
                .into(),
        );
    }

    for (family, name) in [("cpu", "CPU"), ("gpu", "GPU")] {
        if let Some(t) = value(family, SensorKind::Temperature).filter(|t| *t >= 80.0) {
            add(
                &format!("hot-{family}"),
                CheckLevel::Warning,
                format!(
                    "The {name} is already at {t:.0} °C. Give it a few minutes to cool so the \
                     first tests aren't throttled."
                ),
            );
        }
    }
    if value("device", SensorKind::ThermalState).is_some_and(|s| s >= 2.0) {
        add(
            "hot-device",
            CheckLevel::Warning,
            "This device reports it's running hot. Let it cool down before starting.".into(),
        );
    }

    if let Some(b) = inputs.battery.as_ref().filter(|_| on_battery) {
        if b.charge_percent < 30.0 {
            add(
                "low-battery",
                CheckLevel::Warning,
                format!(
                    "The battery is at {:.0}%. A long run may not finish, so plug in or charge \
                     first.",
                    b.charge_percent
                ),
            );
        }
    }
    if on_battery && !battery_test {
        add(
            "on-battery",
            CheckLevel::Info,
            "Running on battery. Laptops often hold back speed to save power, so plug in for \
             peak results."
                .into(),
        );
    }
    if battery_test && !on_battery && inputs.battery.is_some() {
        add(
            "plugged-in",
            CheckLevel::Info,
            "The battery test only runs on battery power. Unplug the charger to include it.".into(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            memory: MemoryInfo {
                total_bytes: (16.0 * GIB) as u64,
                available_bytes: (8.0 * GIB) as u64,
            },
            battery: Some(BatteryInfo {
                charge_percent: 80.0,
                design_capacity_wh: Some(56.0),
                full_capacity_wh: Some(48.0),
                charging: true,
            }),
            power_source: PowerSource::Ac,
            power_saver: Some(false),
            readings: vec![SensorReading::new(
                "cpu",
                SensorKind::Utilization,
                Some(5.0),
            )],
        }
    }

    fn profile(modules: &str) -> Profile {
        Profile::from_toml(&format!("schema = 1\nname = \"t\"\n{modules}")).unwrap()
    }

    fn ids(checks: &[PreflightCheck]) -> Vec<&str> {
        checks.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn a_quiet_plugged_in_laptop_has_nothing_to_report() {
        assert!(check(&profile(""), &inputs()).is_empty());
    }

    #[test]
    fn low_memory_busy_cpu_saver_and_heat_are_warnings() {
        let mut i = inputs();
        i.memory.available_bytes = (1.0 * GIB) as u64;
        i.power_saver = Some(true);
        i.readings = vec![
            SensorReading::new("cpu", SensorKind::Utilization, Some(47.0)),
            SensorReading::new("cpu", SensorKind::Temperature, Some(86.0)),
            SensorReading::new("gpu0", SensorKind::Temperature, Some(70.0)),
            SensorReading::new("device", SensorKind::ThermalState, Some(2.0)),
        ];
        let c = check(&profile(""), &i);
        assert_eq!(
            ids(&c),
            [
                "low-memory",
                "busy-cpu",
                "battery-saver",
                "hot-cpu",
                "hot-device"
            ]
        );
        assert!(c.iter().all(|c| c.level == CheckLevel::Warning));
        assert!(
            c[0].message.contains("1.0 GB of 16.0 GB"),
            "{}",
            c[0].message
        );
        assert!(c[1].message.contains("47%"));
    }

    #[test]
    fn two_gb_free_is_low_even_on_a_small_device() {
        let mut i = inputs();
        i.memory.total_bytes = (6.0 * GIB) as u64;
        i.memory.available_bytes = (1.9 * GIB) as u64;
        assert_eq!(ids(&check(&profile(""), &i)), ["low-memory"]);
    }

    #[test]
    fn battery_checks_depend_on_the_profile() {
        let mut i = inputs();
        i.power_source = PowerSource::Battery;
        i.battery.as_mut().unwrap().charge_percent = 20.0;
        i.battery.as_mut().unwrap().charging = false;
        assert_eq!(ids(&check(&profile(""), &i)), ["low-battery", "on-battery"]);
        // The battery test wants battery power, so that isn't worth a note.
        let battery = profile("[modules.battery-drain]\n");
        assert_eq!(ids(&check(&battery, &i)), ["low-battery"]);
        // Plugged in with the battery test in the profile: say how to include it.
        let plugged = inputs();
        assert_eq!(ids(&check(&battery, &plugged)), ["plugged-in"]);
        let disabled = profile("[modules.battery-drain]\nenabled = false\n");
        assert!(check(&disabled, &plugged).is_empty());
    }

    #[test]
    fn unreadable_sensors_are_not_warnings() {
        let mut i = inputs();
        i.power_saver = None;
        i.readings = vec![
            SensorReading::new("cpu", SensorKind::Utilization, None),
            SensorReading::new("cpu", SensorKind::Temperature, None),
        ];
        assert!(check(&profile(""), &i).is_empty());
    }
}
