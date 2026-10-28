//! Battery: how much charge does local AI cost, and how long would a full battery last?
//!
//! One model generates back to back for `minutes` (10 by default) on battery power, while the
//! run's sensors report charge and, where the device offers it, the drain in watts. Reported:
//!
//! * **Charge used** (%) and **% per 10k tokens**
//! * **Average drain** (W) and **tokens per Wh**, when the device reports watts
//! * **Hours of nonstop generation** from a full battery
//!
//! Plugging the charger in mid-test makes the numbers meaningless, so it fails the test.

use std::time::Instant;

use infermark_sdk::{
    BenchModule, Direction, LiveValue, Metric, ModuleCategory, ModuleDescriptor, ModuleError,
    ModuleOutput, Requirement, RunContext, SensorKind, Series,
};

use super::model_run::{
    context_for, duration_setting, generation_loop, load_options, minutes_label, open, round,
    select_models, Loop, LoopPlan,
};

pub struct BatteryDrain;

/// Charge drops smaller than this (in %) are too coarse to turn into an estimate.
const MIN_DROP_PCT: f64 = 0.5;
/// Loops in a row without a drain reading before we conclude the charger went in.
const PLUGGED_LOOPS: usize = 3;

impl BenchModule for BatteryDrain {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "battery-drain".into(),
            name: "Battery".into(),
            description: "Runs a model on battery power to measure the charge it uses and how \
                          many hours of nonstop generation a full battery gives."
                .into(),
            category: ModuleCategory::Battery,
            requirements: vec![Requirement::Battery, Requirement::OnBatteryPower],
            estimated_seconds: 600,
        }
    }

    /// Settings: `minutes` (default 10), `prompt_tokens` (128), `gen_tokens` (128).
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let plan = LoopPlan {
            prompt_tokens: ctx.setting_u64("prompt_tokens", 128).clamp(16, 8192) as u32,
            gen_tokens: ctx.setting_u64("gen_tokens", 128).clamp(8, 4096) as u32,
            duration: duration_setting(ctx, 10.0),
        };
        let context = context_for(plan.prompt_tokens, plan.gen_tokens);
        let mut out = ModuleOutput::default();
        let sel = select_models(ctx, 1, context, &mut Vec::new())?;
        let c = sel.candidates[0].clone();
        let mut backend = open(&sel, &mut out.notes)?;

        let charge = || ctx.sensors.value("battery", SensorKind::BatteryCharge);
        let drain = || ctx.sensors.value("battery", SensorKind::Power);
        let Some(start_pct) = charge().or(ctx.hardware.battery.as_ref().map(|b| b.charge_percent))
        else {
            return Err(ModuleError::Skipped(
                "the battery charge can't be read on this device".into(),
            ));
        };
        let mut log = BatteryLog::new(start_pct, drain().is_some());
        let total = plan.duration.as_secs_f64();
        let started = Instant::now();
        ctx.report(0.0, format!("{}: loading", c.name));
        generation_loop(
            backend.as_mut(),
            &c,
            &load_options(ctx, context),
            plan,
            ctx,
            &mut |l: &Loop| {
                log.record(l, charge(), drain())?;
                let elapsed = started.elapsed().as_secs_f64();
                ctx.report_live(
                    elapsed / total,
                    format!(
                        "{}: {} of {} on battery",
                        c.name,
                        minutes_label(elapsed),
                        minutes_label(total)
                    ),
                    LiveValue::new("Battery charge", round(log.end_pct, 1), "%"),
                );
                Ok(())
            },
        )?;

        let capacity = ctx
            .hardware
            .battery
            .as_ref()
            .and_then(|b| b.full_capacity_wh.or(b.design_capacity_wh));
        log.summarize(&mut out, capacity);
        out.notes.push(format!(
            "{}: {} tokens over {}, charge {:.1}% → {:.1}%.",
            c.name,
            log.tokens,
            minutes_label(log.seconds),
            log.start_pct,
            log.end_pct
        ));
        Ok(out)
    }
}

/// Charge and drain readings collected loop by loop.
#[derive(Debug, Clone, PartialEq)]
struct BatteryLog {
    start_pct: f64,
    end_pct: f64,
    /// Seconds of generation (since the loop started).
    seconds: f64,
    tokens: u32,
    /// Watts read after each loop, when readable.
    drain_w: Vec<f64>,
    /// Whether drain was readable when the test started (on battery, so it should be).
    drain_expected: bool,
    unreadable_in_a_row: usize,
    charge: Vec<(f64, f64)>,
}

impl BatteryLog {
    fn new(start_pct: f64, drain_expected: bool) -> Self {
        Self {
            start_pct,
            end_pct: start_pct,
            seconds: 0.0,
            tokens: 0,
            drain_w: Vec::new(),
            drain_expected,
            unreadable_in_a_row: 0,
            charge: vec![(0.0, start_pct)],
        }
    }

    /// Add one loop's readings. Fails when the device is clearly charging again.
    fn record(
        &mut self,
        l: &Loop,
        charge: Option<f64>,
        drain: Option<f64>,
    ) -> Result<(), ModuleError> {
        self.seconds = l.t;
        self.tokens += l.tokens;
        if let Some(pct) = charge {
            self.end_pct = pct;
            self.charge.push((l.t, round(pct, 2)));
        }
        match drain {
            Some(w) => {
                self.drain_w.push(w);
                self.unreadable_in_a_row = 0;
            }
            None => self.unreadable_in_a_row += 1,
        }
        let charging = self.end_pct > self.start_pct + MIN_DROP_PCT
            || (self.drain_expected && self.unreadable_in_a_row >= PLUGGED_LOOPS);
        if charging {
            return Err(ModuleError::Failed(
                "the charger was plugged in during the test. Unplug it and run the battery test again."
                    .into(),
            ));
        }
        Ok(())
    }

    fn summarize(&self, out: &mut ModuleOutput, capacity_wh: Option<f64>) {
        let used = (self.start_pct - self.end_pct).max(0.0);
        let hours = self.seconds / 3600.0;
        let avg_w = (!self.drain_w.is_empty())
            .then(|| self.drain_w.iter().sum::<f64>() / self.drain_w.len() as f64);
        let pct_ok = used >= MIN_DROP_PCT && hours > 0.0;

        // Hours from a full battery: watts and capacity are more precise than a small % drop.
        let hours_nonstop = match (avg_w, capacity_wh) {
            (Some(w), Some(cap)) if w > 0.0 => Some(cap / w),
            _ if pct_ok => Some(100.0 / (used / hours)),
            _ => None,
        };

        out.metrics.push(Metric::new(
            "charge_used_pct",
            "Charge used",
            round(used, 2),
            "%",
            Direction::LowerIsBetter,
        ));
        if let Some(h) = hours_nonstop {
            out.metrics.push(Metric::new(
                "hours_nonstop",
                "Hours of nonstop generation (full battery)",
                round(h, 1),
                "h",
                Direction::HigherIsBetter,
            ));
        }
        if pct_ok && self.tokens > 0 {
            out.metrics.push(Metric::new(
                "pct_per_10k_tokens",
                "Battery per 10k tokens",
                round(used / self.tokens as f64 * 10_000.0, 2),
                "%",
                Direction::LowerIsBetter,
            ));
        }
        if let Some(w) = avg_w {
            out.metrics.push(Metric::new(
                "avg_drain_w",
                "Average drain",
                round(w, 1),
                "W",
                Direction::LowerIsBetter,
            ));
            if w > 0.0 && hours > 0.0 {
                out.metrics.push(Metric::new(
                    "tokens_per_wh",
                    "Tokens per Wh",
                    round(self.tokens as f64 / (w * hours), 0),
                    "tok/Wh",
                    Direction::HigherIsBetter,
                ));
            }
        }
        out.metrics.push(Metric::new(
            "tokens",
            "Tokens written",
            self.tokens as f64,
            "",
            Direction::Neutral,
        ));
        out.series.push(Series {
            key: "charge".into(),
            label: "Battery charge".into(),
            x_unit: "s".into(),
            y_unit: "%".into(),
            points: self.charge.clone(),
        });
        if !pct_ok && avg_w.is_none() {
            out.notes.push(format!(
                "The charge only dropped {used:.1}%, which is too little for an estimate. Run the battery test for longer."
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(t: f64) -> Loop {
        Loop {
            t,
            tokens_per_sec: 40.0,
            tokens: 128,
        }
    }

    fn get(out: &ModuleOutput, key: &str) -> Option<f64> {
        out.metrics.iter().find(|m| m.key == key).map(|m| m.value)
    }

    #[test]
    fn drain_in_watts_and_percent_gives_every_metric() {
        // 10 minutes, 80% -> 76%, about 12 W, 50 Wh battery.
        let mut log = BatteryLog::new(80.0, true);
        for i in 1..=200 {
            let t = i as f64 * 3.0;
            log.record(&l(t), Some(80.0 - 4.0 * t / 600.0), Some(12.0))
                .unwrap();
        }
        let mut out = ModuleOutput::default();
        log.summarize(&mut out, Some(50.0));
        assert_eq!(get(&out, "charge_used_pct"), Some(4.0));
        assert_eq!(get(&out, "tokens"), Some(25_600.0));
        assert_eq!(get(&out, "pct_per_10k_tokens"), Some(1.56));
        assert_eq!(get(&out, "avg_drain_w"), Some(12.0));
        // 25,600 tokens / (12 W × 1/6 h) = 12,800 tok/Wh
        assert_eq!(get(&out, "tokens_per_wh"), Some(12_800.0));
        // 50 Wh / 12 W
        assert_eq!(get(&out, "hours_nonstop"), Some(4.2));
        assert_eq!(out.series[0].points.len(), 201);
        assert!(out.notes.is_empty());
    }

    #[test]
    fn without_watts_hours_come_from_the_charge_drop() {
        // Phones without a current sensor: 3% in 6 minutes -> 100% lasts 200 minutes.
        let mut log = BatteryLog::new(60.0, false);
        log.record(&l(180.0), Some(58.5), None).unwrap();
        log.record(&l(360.0), Some(57.0), None).unwrap();
        let mut out = ModuleOutput::default();
        log.summarize(&mut out, None);
        assert_eq!(get(&out, "hours_nonstop"), Some(3.3));
        assert_eq!(get(&out, "avg_drain_w"), None);
        assert_eq!(get(&out, "tokens_per_wh"), None);
    }

    #[test]
    fn a_tiny_drop_without_watts_gives_no_estimate() {
        let mut log = BatteryLog::new(90.0, false);
        log.record(&l(60.0), Some(89.8), None).unwrap();
        let mut out = ModuleOutput::default();
        log.summarize(&mut out, Some(50.0));
        assert_eq!(get(&out, "hours_nonstop"), None);
        assert_eq!(get(&out, "pct_per_10k_tokens"), None);
        assert!(out.notes[0].contains("too little"));
    }

    #[test]
    fn plugging_in_fails_the_test() {
        // Drain stops being reported (the battery isn't discharging any more)...
        let mut log = BatteryLog::new(70.0, true);
        log.record(&l(3.0), Some(70.0), Some(10.0)).unwrap();
        log.record(&l(6.0), Some(70.0), None).unwrap();
        log.record(&l(9.0), Some(70.0), None).unwrap();
        let err = log.record(&l(12.0), Some(70.0), None).unwrap_err();
        assert!(err.to_string().contains("charger was plugged in"));

        // ...or the charge goes up.
        let mut log = BatteryLog::new(70.0, false);
        let err = log.record(&l(3.0), Some(71.0), None).unwrap_err();
        assert!(err.to_string().contains("charger"));
    }

    #[test]
    fn needs_a_battery_and_battery_power() {
        let d = BatteryDrain.descriptor();
        assert!(d.requirements.contains(&Requirement::OnBatteryPower));
        assert!(d.requirements.contains(&Requirement::Battery));
    }
}
