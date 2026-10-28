//! Endurance: does speed hold up once the device is hot?
//!
//! One model generates back to back for `minutes` (5 by default). A short benchmark catches a
//! device at its coolest; this one keeps going until the fans, the power limit or the phone's
//! thermal governor have had their say. Reported:
//!
//! * **Peak** (tok/s): the best single generation
//! * **Sustained** (tok/s): the median of the last quarter of generations
//! * **Stability** (%): sustained ÷ peak; **throttling** is the rest
//! * **Burst** (tok/s): what `inference-perf` measured for the same model earlier in the run

use std::time::Instant;

use infermark_sdk::{
    BenchModule, Direction, LiveValue, Metric, ModuleCategory, ModuleDescriptor, ModuleError,
    ModuleOutput, RunContext, SensorKind, SensorSnapshot, Series,
};

use super::model_run::{
    context_for, duration_setting, generation_loop, load_options, median, minutes_label, open,
    round, select_models, Loop, LoopPlan,
};

pub struct SustainedLoad;

impl BenchModule for SustainedLoad {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "sustained-load".into(),
            name: "Endurance".into(),
            description: "Keeps a model writing for several minutes to see whether speed holds up \
                          as the device heats up."
                .into(),
            category: ModuleCategory::Endurance,
            requirements: vec![],
            estimated_seconds: 300,
        }
    }

    /// Settings: `minutes` (default 5), `prompt_tokens` (128), `gen_tokens` (128), `model`
    /// (a model key such as `qwen3-0.6b-q4`; default: the smallest model that fits).
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let plan = LoopPlan {
            prompt_tokens: ctx.setting_u64("prompt_tokens", 128).clamp(16, 8192) as u32,
            gen_tokens: ctx.setting_u64("gen_tokens", 128).clamp(8, 4096) as u32,
            duration: duration_setting(ctx, 5.0),
        };
        let context = context_for(plan.prompt_tokens, plan.gen_tokens);
        let mut out = ModuleOutput::default();
        let wanted = ctx.settings.get("model").and_then(|v| v.as_str());
        let mut sel = select_models(
            ctx,
            if wanted.is_some() { usize::MAX } else { 1 },
            context,
            &mut Vec::new(),
        )?;
        if let Some(key) = wanted {
            sel.candidates.retain(|c| c.key == key);
            if sel.candidates.is_empty() {
                return Err(ModuleError::Skipped(format!(
                    "model `{key}` isn't downloaded or doesn't fit this device"
                )));
            }
        }
        let c = sel.candidates[0].clone();
        let mut backend = open(&sel, &mut out.notes)?;

        let before = Thermals::read(ctx.sensors);
        let total = plan.duration.as_secs_f64();
        let started = Instant::now();
        ctx.report(0.0, format!("{}: loading", c.name));
        let loops = generation_loop(
            backend.as_mut(),
            &c,
            &load_options(ctx, context),
            plan,
            ctx,
            &mut |l: &Loop| {
                let elapsed = started.elapsed().as_secs_f64();
                ctx.report_live(
                    elapsed / total,
                    format!(
                        "{}: {} of {}",
                        c.name,
                        minutes_label(elapsed),
                        minutes_label(total)
                    ),
                    LiveValue::new(
                        format!("{} · Sustained", c.name),
                        round(l.tokens_per_sec, 1),
                        "tok/s",
                    ),
                );
                Ok(())
            },
        )?;
        let after = Thermals::read(ctx.sensors);

        let burst = ctx.earlier_metric("inference-perf", &format!("{}/gen_tps", c.key));
        summarize(&mut out, &c.name, &loops, burst);
        out.notes.extend(before.change_note(&after));
        Ok(out)
    }
}

/// Peak, sustained and stability from the loops; adds metrics, the speed series and a note.
fn summarize(out: &mut ModuleOutput, model: &str, loops: &[Loop], burst: Option<f64>) {
    let speeds: Vec<f64> = loops.iter().map(|l| l.tokens_per_sec).collect();
    let peak = speeds.iter().cloned().fold(0.0, f64::max);
    let tail = &speeds[speeds.len() - speeds.len().div_ceil(4)..];
    let sustained = median(tail);
    let stability = if peak > 0.0 {
        (sustained / peak * 100.0).min(100.0)
    } else {
        0.0
    };
    let tokens: u32 = loops.iter().map(|l| l.tokens).sum();
    let minutes = loops.last().map_or(0.0, |l| l.t / 60.0);

    out.metrics.extend([
        Metric::new(
            "stability_pct",
            "Stability",
            round(stability, 1),
            "%",
            Direction::HigherIsBetter,
        ),
        Metric::new(
            "throttle_pct",
            "Throttling",
            round(100.0 - stability, 1),
            "%",
            Direction::LowerIsBetter,
        ),
        Metric::new(
            "peak_tps",
            "Peak speed",
            round(peak, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ),
        Metric::new(
            "sustained_tps",
            "Sustained speed",
            round(sustained, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ),
    ]);
    if let Some(b) = burst {
        out.metrics.push(Metric::new(
            "burst_tps",
            "Burst speed (short test)",
            round(b, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ));
    }
    out.metrics.extend([
        Metric::new(
            "loops",
            "Generations",
            loops.len() as f64,
            "",
            Direction::Neutral,
        ),
        Metric::new(
            "tokens",
            "Tokens written",
            tokens as f64,
            "",
            Direction::Neutral,
        ),
    ]);
    out.series.push(Series {
        key: "tps".into(),
        label: "Speed over time".into(),
        x_unit: "s".into(),
        y_unit: "tok/s".into(),
        points: loops.iter().map(|l| (l.t, l.tokens_per_sec)).collect(),
    });
    out.notes.push(format!(
        "{model}: {} generations over {minutes:.1} min, peak {peak:.1} tok/s, last quarter {sustained:.1} tok/s.",
        loops.len()
    ));
    if loops.len() < 8 {
        out.notes.push(
            "Fewer than 8 generations: run it for longer for a steadier endurance result.".into(),
        );
    }
}

/// Temperatures (or the coarse thermal state phones give) at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Thermals {
    cpu: Option<f64>,
    gpu: Option<f64>,
    state: Option<f64>,
}

impl Thermals {
    fn read(s: &SensorSnapshot) -> Self {
        Self {
            cpu: s.value("cpu", SensorKind::Temperature),
            gpu: s.value("gpu", SensorKind::Temperature),
            state: s.value("device", SensorKind::ThermalState),
        }
    }

    /// e.g. "CPU went from 52 °C to 78 °C." Nothing when no temperature was readable.
    fn change_note(&self, after: &Self) -> Option<String> {
        let mut parts = Vec::new();
        for (name, a, b) in [("CPU", self.cpu, after.cpu), ("GPU", self.gpu, after.gpu)] {
            if let (Some(a), Some(b)) = (a, b) {
                parts.push(format!("{name} went from {a:.0} °C to {b:.0} °C"));
            }
        }
        if let (Some(a), Some(b)) = (self.state, after.state) {
            parts.push(format!(
                "thermal state went from {} to {}",
                state_name(a),
                state_name(b)
            ));
        }
        (!parts.is_empty()).then(|| {
            let mut s = parts.join("; ");
            s.replace_range(..1, &s[..1].to_uppercase());
            format!("{s}.")
        })
    }
}

fn state_name(v: f64) -> &'static str {
    match v.round() as i64 {
        i64::MIN..=0 => "nominal",
        1 => "fair",
        2 => "serious",
        _ => "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loops(speeds: &[f64]) -> Vec<Loop> {
        speeds
            .iter()
            .enumerate()
            .map(|(i, s)| Loop {
                t: (i + 1) as f64 * 3.0,
                tokens_per_sec: *s,
                tokens: 128,
            })
            .collect()
    }

    #[test]
    fn a_throttling_device_loses_stability() {
        let mut out = ModuleOutput::default();
        // 40 tok/s cold, settling at 30 once hot.
        let speeds = [
            40.0, 39.5, 38.0, 36.0, 33.0, 31.0, 30.0, 30.0, 30.5, 29.5, 30.0, 30.0,
        ];
        summarize(&mut out, "Fake", &loops(&speeds), Some(41.0));
        let get = |k: &str| out.metrics.iter().find(|m| m.key == k).unwrap().value;
        assert_eq!(get("peak_tps"), 40.0);
        assert_eq!(get("sustained_tps"), 30.0);
        assert_eq!(get("stability_pct"), 75.0);
        assert_eq!(get("throttle_pct"), 25.0);
        assert_eq!(get("burst_tps"), 41.0);
        assert_eq!(get("tokens"), 12.0 * 128.0);
        assert_eq!(out.series[0].points.len(), 12);
        assert_eq!(out.notes.len(), 1, "long enough: no short-run warning");
    }

    #[test]
    fn a_steady_device_is_near_100_and_short_runs_are_flagged() {
        let mut out = ModuleOutput::default();
        summarize(&mut out, "Fake", &loops(&[45.0, 44.8, 45.1]), None);
        let get = |k: &str| out.metrics.iter().find(|m| m.key == k).unwrap().value;
        assert!(get("stability_pct") > 99.0);
        assert!(get("throttle_pct") < 1.0);
        assert!(!out.metrics.iter().any(|m| m.key == "burst_tps"));
        assert!(out.notes.iter().any(|n| n.contains("Fewer than 8")));
    }

    #[test]
    fn temperature_note_only_when_readable() {
        let none = Thermals {
            cpu: None,
            gpu: None,
            state: None,
        };
        assert_eq!(none.change_note(&none), None);
        let cold = Thermals {
            cpu: Some(52.0),
            gpu: None,
            state: Some(0.0),
        };
        let hot = Thermals {
            cpu: Some(78.4),
            gpu: None,
            state: Some(2.0),
        };
        assert_eq!(
            cold.change_note(&hot).unwrap(),
            "CPU went from 52 °C to 78 °C; thermal state went from nominal to serious."
        );
    }
}
