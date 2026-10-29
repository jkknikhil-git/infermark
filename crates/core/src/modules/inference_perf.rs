//! Inference speed for every model that passes the hardware gate:
//!
//! * **Prompt processing** (tokens/s): how fast a ~512-token prompt is read
//! * **Generation** (tokens/s): how fast the answer is written (128 tokens)
//! * **Time to first token** (ms): how long until text starts appearing
//! * **Load time** (s): how long the model takes to load into memory
//!
//! Each model gets warm-up runs, then `run.repeats` measured runs; results are medians.
//! Every prompt starts with a different session number so no backend can serve it from cache.

use infermark_sdk::{
    Backend, BenchModule, Direction, GenerationStats, LiveValue, LoadOptions, Metric,
    ModuleCategory, ModuleDescriptor, ModuleError, ModuleOutput, RunContext, Series,
};

use super::model_run::{
    context_for, load_options, median, open, request, round, select_models, threads_label,
    Candidate,
};

pub struct InferencePerf;

#[derive(Debug, Clone, Copy)]
struct Plan {
    prompt_tokens: u32,
    gen_tokens: u32,
    context: u32,
    warmups: u32,
    repeats: u32,
}

impl BenchModule for InferencePerf {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "inference-perf".into(),
            name: "Inference speed".into(),
            description: "How fast each model reads your prompt and writes its answer \
                          (tokens per second), how long until the first word appears, and load time."
                .into(),
            category: ModuleCategory::Inference,
            requirements: vec![],
            estimated_seconds: 90,
        }
    }

    /// Settings: `max_models` (default 3), `prompt_tokens` (512), `gen_tokens` (128).
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let prompt_tokens = ctx.setting_u64("prompt_tokens", 512).clamp(16, 32768) as u32;
        let gen_tokens = ctx.setting_u64("gen_tokens", 128).clamp(8, 4096) as u32;
        let plan = Plan {
            prompt_tokens,
            gen_tokens,
            context: context_for(prompt_tokens, gen_tokens),
            warmups: ctx.run.warmups,
            repeats: ctx.run.repeats.max(1),
        };
        let max_models = ctx.setting_u64("max_models", 3).max(1) as usize;

        let mut out = ModuleOutput::default();
        let sel = select_models(ctx, max_models, plan.context, &mut out.notes)?;
        let mut backend = open(&sel, &mut out.notes)?;
        let candidates = sel.candidates;

        let total = candidates.len();
        let mut failures = Vec::new();
        for (i, c) in candidates.iter().enumerate() {
            let report = |f: f64, msg: &str, live: Option<LiveValue>| {
                let fraction = (i as f64 + f) / total as f64;
                let msg = format!("{}: {msg}", c.name);
                match live {
                    Some(v) => ctx.report_live(fraction, msg, v),
                    None => ctx.report(fraction, msg),
                }
            };
            let options = load_options(ctx, plan.context);
            match measure(backend.as_mut(), c, &options, plan, ctx, &report) {
                Ok(samples) => push_results(&mut out, c, &samples, &threads_label(&options)),
                Err(ModuleError::Cancelled) => return Err(ModuleError::Cancelled),
                Err(e) => {
                    out.notes.push(format!("{} failed: {e}", c.name));
                    failures.push(format!("{}: {e}", c.name));
                }
            }
        }
        if failures.len() == total {
            return Err(ModuleError::Failed(failures.join("; ")));
        }
        Ok(out)
    }
}

fn measure(
    backend: &mut dyn Backend,
    c: &Candidate,
    options: &LoadOptions,
    plan: Plan,
    ctx: &RunContext<'_>,
    report: &dyn Fn(f64, &str, Option<LiveValue>),
) -> Result<Vec<GenerationStats>, ModuleError> {
    report(0.0, "loading", None);
    backend
        .load(&c.spec, options)
        .map_err(ModuleError::Failed)?;
    let steps = (plan.warmups + plan.repeats) as f64;
    let mut samples = Vec::new();
    let mut session = 0;
    let result = (|| {
        for w in 0..plan.warmups {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            report((w as f64 + 0.5) / (steps + 0.5), "warming up", None);
            session += 1;
            backend
                .generate(
                    &request(plan.prompt_tokens.min(128), 16, session),
                    &mut |_| {},
                )
                .map_err(ModuleError::Failed)?;
        }
        for r in 0..plan.repeats {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            report(
                (plan.warmups as f64 + r as f64 + 0.5) / (steps + 0.5),
                &format!("run {} of {}", r + 1, plan.repeats),
                None,
            );
            session += 1;
            let stats = backend
                .generate(
                    &request(plan.prompt_tokens, plan.gen_tokens, session),
                    &mut |_| {},
                )
                .map_err(ModuleError::Failed)?;
            report(
                (plan.warmups as f64 + r as f64 + 1.0) / (steps + 0.5),
                &format!("run {} of {}", r + 1, plan.repeats),
                Some(LiveValue::new(
                    format!("{} · Generation", c.name),
                    round(stats.generation_tokens_per_sec(), 1),
                    "tok/s",
                )),
            );
            samples.push(stats);
        }
        Ok(())
    })();
    backend.unload();
    result.map(|_| samples)
}

fn push_results(out: &mut ModuleOutput, c: &Candidate, samples: &[GenerationStats], threads: &str) {
    let pick =
        |f: &dyn Fn(&GenerationStats) -> f64| -> Vec<f64> { samples.iter().map(f).collect() };
    let gen = pick(&|s| s.generation_tokens_per_sec());
    let pp = pick(&|s| s.prompt_tokens_per_sec());
    let ttft = pick(&|s| s.time_to_first_token_ms);
    let load_s = samples.first().map(|s| s.load_ms / 1000.0).unwrap_or(0.0);
    let key = |k: &str| format!("{}/{k}", c.key);
    let label = |l: &str| format!("{} · {l}", c.name);
    out.metrics.extend([
        Metric::new(
            key("gen_tps"),
            label("Generation"),
            round(median(&gen), 1),
            "tok/s",
            Direction::HigherIsBetter,
        ),
        Metric::new(
            key("prompt_tps"),
            label("Prompt processing"),
            round(median(&pp), 1),
            "tok/s",
            Direction::HigherIsBetter,
        ),
        Metric::new(
            key("ttft_ms"),
            label("Time to first token"),
            round(median(&ttft), 0),
            "ms",
            Direction::LowerIsBetter,
        ),
        Metric::new(
            key("load_s"),
            label("Load time"),
            round(load_s, 2),
            "s",
            Direction::LowerIsBetter,
        ),
    ]);
    out.series.push(Series {
        key: key("gen_tps"),
        label: label("Generation speed per run"),
        x_unit: "run".into(),
        y_unit: "tok/s".into(),
        points: gen
            .iter()
            .enumerate()
            .map(|(i, v)| ((i + 1) as f64, round(*v, 2)))
            .collect(),
    });
    let prompt_tokens = samples.first().map(|s| s.prompt_tokens).unwrap_or(0);
    let out_tokens = samples.first().map(|s| s.output_tokens).unwrap_or(0);
    out.notes.push(format!(
        "{}: {prompt_tokens}-token prompt, {out_tokens} tokens generated on {threads}, median of {} runs (generation spread {:.1}-{:.1} tok/s).",
        c.name,
        samples.len(),
        gen.iter().cloned().fold(f64::INFINITY, f64::min),
        gen.iter().cloned().fold(0.0, f64::max),
    ));
}

#[cfg(test)]
mod tests {
    use super::super::model_run::testing::{candidate, FakeBackend, Harness};
    use super::*;

    #[test]
    fn measures_and_reports_medians() {
        let h = Harness::new("[run]\nrepeats = 3\nwarmups = 1\n");
        let ctx = h.ctx();
        let c = candidate();
        let plan = Plan {
            prompt_tokens: 512,
            gen_tokens: 128,
            context: 1024,
            warmups: 1,
            repeats: 3,
        };
        let mut backend = FakeBackend::new(|_| 20.0);
        let samples = measure(
            &mut backend,
            &c,
            &LoadOptions::default(),
            plan,
            &ctx,
            &|_, _, _| {},
        )
        .unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(backend.calls, 4, "1 warm-up + 3 measured");
        assert!(!backend.loaded, "model is unloaded afterwards");

        let mut out = ModuleOutput::default();
        push_results(&mut out, &c, &samples, "4 threads");
        let get = |k: &str| out.metrics.iter().find(|m| m.key == k).unwrap().value;
        assert_eq!(get("fake-1b/gen_tps"), 20.0);
        assert_eq!(get("fake-1b/prompt_tps"), 100.0);
        assert_eq!(get("fake-1b/ttft_ms"), 303.0);
        assert_eq!(get("fake-1b/load_s"), 1.5);
        assert_eq!(out.series[0].points.len(), 3);
    }

    #[test]
    fn reports_each_measured_run_live() {
        let h = Harness::new("");
        let ctx = h.ctx();
        let plan = Plan {
            prompt_tokens: 64,
            gen_tokens: 8,
            context: 1024,
            warmups: 0,
            repeats: 2,
        };
        let live = std::sync::Mutex::new(Vec::new());
        measure(
            &mut FakeBackend::new(|_| 25.0),
            &candidate(),
            &LoadOptions::default(),
            plan,
            &ctx,
            &|_, _, v| live.lock().unwrap().extend(v),
        )
        .unwrap();
        let live = live.into_inner().unwrap();
        assert_eq!(live.len(), 2);
        assert_eq!(live[0].value, 25.0);
        assert_eq!(live[0].label, "Fake 1B · Generation");
    }

    #[test]
    fn cancel_stops_between_runs() {
        let h = Harness::new("");
        h.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let ctx = h.ctx();
        let plan = Plan {
            prompt_tokens: 64,
            gen_tokens: 8,
            context: 1024,
            warmups: 1,
            repeats: 2,
        };
        let mut backend = FakeBackend::new(|_| 20.0);
        let err = measure(
            &mut backend,
            &candidate(),
            &LoadOptions::default(),
            plan,
            &ctx,
            &|_, _, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, ModuleError::Cancelled));
        assert_eq!(backend.calls, 0);
        assert!(!backend.loaded);
    }
}
