//! CPU threads: how many threads should llama.cpp use on this processor?
//!
//! More isn't always faster. On phones and Intel hybrid chips the efficiency cores can hold the
//! fast ones back, and on most CPUs hyperthreads add little to generation, which is limited by
//! memory bandwidth. So the smallest model that fits runs on the CPU alone (GPU off) with each
//! sensible thread count:
//!
//! * the fast cores only, when the CPU has more than one kind of core
//! * every physical core
//! * every thread, when the CPU has more threads than cores

use infermark_sdk::{
    Backend, BenchModule, CpuInfo, Direction, LiveValue, LoadOptions, Metric, ModuleCategory,
    ModuleDescriptor, ModuleError, ModuleOutput, RunContext, Series,
};

use super::model_run::{
    context_for, load_options, median, open, request, round, select_models, Candidate,
};

pub struct ThreadSweep;

impl BenchModule for ThreadSweep {
    fn descriptor(&self) -> ModuleDescriptor {
        ModuleDescriptor {
            id: "thread-sweep".into(),
            name: "CPU threads".into(),
            description: "Runs a model on the CPU alone with different thread counts to find \
                          the fastest setting for this processor."
                .into(),
            category: ModuleCategory::Inference,
            requirements: vec![],
            estimated_seconds: 90,
        }
    }

    /// Settings: `prompt_tokens` (128), `gen_tokens` (64), `repeats` (2 per thread count).
    fn run(&self, ctx: &RunContext<'_>) -> Result<ModuleOutput, ModuleError> {
        let prompt_tokens = ctx.setting_u64("prompt_tokens", 128).clamp(16, 4096) as u32;
        let gen_tokens = ctx.setting_u64("gen_tokens", 64).clamp(8, 1024) as u32;
        let repeats = ctx.setting_u64("repeats", 2).clamp(1, 10) as u32;
        let counts = thread_counts(&ctx.hardware.cpu);
        if counts.len() < 2 {
            return Err(ModuleError::Skipped(
                "this CPU has only one sensible thread count to test".into(),
            ));
        }
        let context = context_for(prompt_tokens, gen_tokens);
        let mut out = ModuleOutput::default();
        let sel = select_models(ctx, 1, context, &mut Vec::new())?;
        let c = sel.candidates[0].clone();
        let mut backend = open(&sel, &mut out.notes)?;

        let mut results = Vec::new();
        for (i, (threads, what)) in counts.iter().enumerate() {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            ctx.report(
                i as f64 / counts.len() as f64,
                format!("{}: {threads} threads ({what})", c.name),
            );
            let options = LoadOptions {
                threads: Some(*threads),
                gpu_layers: Some(0),
                ..load_options(ctx, context)
            };
            let (gen, prompt) = measure(
                backend.as_mut(),
                &c,
                &options,
                prompt_tokens,
                gen_tokens,
                repeats,
                ctx,
            )?;
            ctx.report_live(
                (i + 1) as f64 / counts.len() as f64,
                format!("{}: {threads} threads ({what})", c.name),
                LiveValue::new(format!("{} · CPU only", c.name), round(gen, 1), "tok/s"),
            );
            results.push(Sweep {
                threads: *threads,
                what,
                gen_tps: gen,
                prompt_tps: prompt,
            });
        }
        summarize(&mut out, &c.name, &results);
        Ok(out)
    }
}

/// Thread counts worth trying on `cpu`, fewest first, each with what it means.
fn thread_counts(cpu: &CpuInfo) -> Vec<(u32, &'static str)> {
    let mut out: Vec<(u32, &'static str)> = Vec::new();
    let mut add = |n: u32, what| {
        if n > 0 && !out.iter().any(|(m, _)| *m == n) {
            out.push((n, what));
        }
    };
    if let Some(fast) = cpu.fast_cores() {
        add(fast, "fast cores");
    }
    if let Some(physical) = cpu.physical_cores {
        add(physical, "all cores");
    }
    add(cpu.logical_cores, "all threads");
    out.sort_by_key(|(n, _)| *n);
    out
}

/// Median generation and prompt speed for one thread count (one warm-up first).
fn measure(
    backend: &mut dyn Backend,
    c: &Candidate,
    options: &LoadOptions,
    prompt_tokens: u32,
    gen_tokens: u32,
    repeats: u32,
    ctx: &RunContext<'_>,
) -> Result<(f64, f64), ModuleError> {
    backend
        .load(&c.spec, options)
        .map_err(ModuleError::Failed)?;
    let result = (|| {
        let mut gen = Vec::new();
        let mut prompt = Vec::new();
        for session in 0..=repeats {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            let stats = backend
                .generate(&request(prompt_tokens, gen_tokens, session), &mut |_| {})
                .map_err(ModuleError::Failed)?;
            // Session 0 is the warm-up.
            if session > 0 {
                gen.push(stats.generation_tokens_per_sec());
                prompt.push(stats.prompt_tokens_per_sec());
            }
        }
        Ok((median(&gen), median(&prompt)))
    })();
    backend.unload();
    result
}

struct Sweep {
    threads: u32,
    what: &'static str,
    gen_tps: f64,
    prompt_tps: f64,
}

fn summarize(out: &mut ModuleOutput, model: &str, results: &[Sweep]) {
    for r in results {
        out.metrics.push(Metric::new(
            format!("t{}_gen_tps", r.threads),
            format!("{} threads · Generation", r.threads),
            round(r.gen_tps, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ));
        out.metrics.push(Metric::new(
            format!("t{}_prompt_tps", r.threads),
            format!("{} threads · Prompt processing", r.threads),
            round(r.prompt_tps, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ));
    }
    let Some(best) = results
        .iter()
        .max_by(|a, b| a.gen_tps.total_cmp(&b.gen_tps))
    else {
        return;
    };
    out.metrics.insert(
        0,
        Metric::new(
            "best_threads",
            "Fastest thread count",
            best.threads as f64,
            "",
            Direction::Neutral,
        ),
    );
    out.metrics.insert(
        1,
        Metric::new(
            "best_gen_tps",
            "Generation on the CPU alone",
            round(best.gen_tps, 1),
            "tok/s",
            Direction::HigherIsBetter,
        ),
    );
    out.series.push(Series {
        key: "gen_tps_by_threads".into(),
        label: "Generation speed by thread count".into(),
        x_unit: "threads".into(),
        y_unit: "tok/s".into(),
        points: results
            .iter()
            .map(|r| (r.threads as f64, round(r.gen_tps, 2)))
            .collect(),
    });
    let others: Vec<String> = results
        .iter()
        .filter(|r| r.threads != best.threads)
        .map(|r| format!("{} threads ({}) {:.1} tok/s", r.threads, r.what, r.gen_tps))
        .collect();
    out.notes.push(format!(
        "{model} on the CPU alone (GPU off): fastest with {} threads ({}), {:.1} tok/s; {}.",
        best.threads,
        best.what,
        best.gen_tps,
        others.join(", ")
    ));
}

#[cfg(test)]
mod tests {
    use super::super::model_run::testing::{candidate, FakeBackend, Harness};
    use super::*;
    use infermark_sdk::CpuCluster;

    fn cpu(physical: u32, logical: u32, clusters: &[(u32, u32)]) -> CpuInfo {
        CpuInfo {
            brand: "c".into(),
            vendor: String::new(),
            physical_cores: Some(physical),
            logical_cores: logical,
            frequency_mhz: None,
            clusters: clusters
                .iter()
                .map(|(p, l)| CpuCluster {
                    physical_cores: *p,
                    logical_cores: *l,
                    max_mhz: None,
                })
                .collect(),
        }
    }

    #[test]
    fn thread_counts_follow_the_core_layout() {
        // This laptop: 4 cores, 8 threads, one kind of core.
        assert_eq!(
            thread_counts(&cpu(4, 8, &[(4, 8)])),
            [(4, "all cores"), (8, "all threads")]
        );
        // A phone: 1 prime + 3 big + 4 little.
        assert_eq!(
            thread_counts(&cpu(8, 8, &[(1, 1), (3, 3), (4, 4)])),
            [(4, "fast cores"), (8, "all cores")]
        );
        // Intel hybrid: 6 P-cores with hyperthreads + 8 E-cores.
        assert_eq!(
            thread_counts(&cpu(14, 20, &[(6, 12), (8, 8)])),
            [(6, "fast cores"), (14, "all cores"), (20, "all threads")]
        );
        // Nothing to compare.
        assert_eq!(thread_counts(&cpu(4, 4, &[])), [(4, "all cores")]);
    }

    #[test]
    fn measures_cpu_only_and_unloads() {
        let h = Harness::new("");
        let mut backend = FakeBackend::new(|call| if call == 1 { 1.0 } else { 18.0 });
        let options = LoadOptions {
            threads: Some(4),
            gpu_layers: Some(0),
            ..LoadOptions::default()
        };
        let (gen, prompt) =
            measure(&mut backend, &candidate(), &options, 128, 64, 2, &h.ctx()).unwrap();
        assert_eq!(backend.calls, 3, "1 warm-up + 2 measured");
        assert_eq!(gen, 18.0, "the slow warm-up isn't counted");
        assert!((prompt - 100.0).abs() < 1e-9, "{prompt}");
        assert!(!backend.loaded);
    }

    #[test]
    fn picks_the_fastest_and_says_so() {
        let mut out = ModuleOutput::default();
        let results = [
            Sweep {
                threads: 4,
                what: "all cores",
                gen_tps: 21.4,
                prompt_tps: 90.0,
            },
            Sweep {
                threads: 8,
                what: "all threads",
                gen_tps: 17.9,
                prompt_tps: 110.0,
            },
        ];
        summarize(&mut out, "Fake", &results);
        let get = |k: &str| out.metrics.iter().find(|m| m.key == k).unwrap().value;
        assert_eq!(get("best_threads"), 4.0);
        assert_eq!(get("best_gen_tps"), 21.4);
        assert_eq!(get("t8_gen_tps"), 17.9);
        assert_eq!(get("t8_prompt_tps"), 110.0);
        assert_eq!(out.series[0].points, [(4.0, 21.4), (8.0, 17.9)]);
        assert!(
            out.notes[0].contains("fastest with 4 threads (all cores), 21.4 tok/s"),
            "{}",
            out.notes[0]
        );
    }
}
