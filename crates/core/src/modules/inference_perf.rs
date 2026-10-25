//! Inference speed for every model that passes the hardware gate:
//!
//! * **Prompt processing** (tokens/s): how fast a ~512-token prompt is read
//! * **Generation** (tokens/s): how fast the answer is written (128 tokens)
//! * **Time to first token** (ms): how long until text starts appearing
//! * **Load time** (s): how long the model takes to load into memory
//!
//! Each model gets warm-up runs, then `run.repeats` measured runs; results are medians.
//! Every prompt starts with a different session number so no backend can serve it from cache.

use std::path::{Path, PathBuf};

use infermark_engine::{open_backend, BackendKind, EngineConfig};
use infermark_models::{assess, gguf, Catalog, ModelShape, ModelStore, Verdict};
use infermark_sdk::{
    Backend, BenchModule, Direction, GenerateRequest, GenerationStats, LoadOptions, Metric,
    ModelSpec, ModuleCategory, ModuleDescriptor, ModuleError, ModuleOutput, RunContext, Series,
};

use crate::settings;

pub struct InferencePerf;

/// A model this run could benchmark.
#[derive(Debug, Clone)]
struct Candidate {
    /// Catalog id, file stem or Ollama tag; used as the metric key prefix.
    key: String,
    name: String,
    params_b: Option<f64>,
    spec: ModelSpec,
    /// File size and shape, when known, for the hardware gate.
    gate_input: Option<(u64, ModelShape)>,
}

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
        let settings = settings::load_or_default();
        let config = settings.engine_config();
        let plan = Plan {
            prompt_tokens: ctx.setting_u64("prompt_tokens", 512).clamp(16, 32768) as u32,
            gen_tokens: ctx.setting_u64("gen_tokens", 128).clamp(8, 4096) as u32,
            context: 0,
            warmups: ctx.run.warmups,
            repeats: ctx.run.repeats.max(1),
        };
        let plan = Plan {
            context: (plan.prompt_tokens + plan.gen_tokens + 128)
                .next_power_of_two()
                .max(1024),
            ..plan
        };
        let max_models = ctx.setting_u64("max_models", 3).max(1) as usize;

        let mut out = ModuleOutput::default();
        let kind = choose_backend(&ctx.run.backend, &config, &settings.models_dir(), ctx)?;
        let mut candidates = match kind {
            BackendKind::LlamaCpp => gguf_candidates(ctx, &settings.models_dir(), &mut out.notes),
            BackendKind::Ollama => ollama_candidates(ctx, &config, &mut out.notes),
        };

        // Hardware gate.
        // The better of copy and triad: a single noisy pass shouldn't make the gate pessimistic.
        let bandwidth = ["copy_gbps", "triad_gbps"]
            .iter()
            .filter_map(|k| ctx.earlier_metric("memory-bandwidth", k))
            .reduce(f64::max);
        candidates.retain(|c| {
            let Some((bytes, shape)) = &c.gate_input else {
                return true;
            };
            let v: Verdict = assess(
                *bytes,
                shape,
                plan.context,
                ctx.hardware,
                &ctx.profile.gate,
                bandwidth,
            );
            if v.runnable(&ctx.profile.gate) {
                true
            } else {
                out.notes
                    .push(format!("Skipped {}: {}.", c.name, v.summary));
                false
            }
        });
        candidates.sort_by(|a, b| {
            a.params_b
                .unwrap_or(f64::MAX)
                .total_cmp(&b.params_b.unwrap_or(f64::MAX))
        });
        if candidates.len() > max_models {
            let rest: Vec<_> = candidates.drain(max_models..).map(|c| c.name).collect();
            out.notes.push(format!(
                "Tested the {max_models} smallest model(s); not tested this time: {}.",
                rest.join(", ")
            ));
        }
        if candidates.is_empty() {
            return Err(ModuleError::Skipped(match kind {
                BackendKind::LlamaCpp => {
                    "no downloaded models fit this profile. Download one on the Models page.".into()
                }
                BackendKind::Ollama => "no Ollama models fit this profile".into(),
            }));
        }

        let mut backend = open_backend(kind, &config).map_err(ModuleError::Failed)?;
        out.notes
            .insert(0, format!("Engine: {} {}", kind.label(), backend.version()));

        let total = candidates.len();
        let mut failures = Vec::new();
        for (i, c) in candidates.iter().enumerate() {
            let report = |f: f64, msg: &str| {
                ctx.report((i as f64 + f) / total as f64, format!("{}: {msg}", c.name))
            };
            let options = LoadOptions {
                context_size: plan.context,
                threads: ctx.run.threads,
                gpu_layers: ctx.run.gpu_layers,
                seed: ctx.run.seed,
            };
            match measure(backend.as_mut(), c, &options, plan, ctx, &report) {
                Ok(samples) => push_results(&mut out, c, &samples),
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

/// `auto` prefers llama.cpp when it is installed and has a model to run, then Ollama.
fn choose_backend(
    setting: &str,
    config: &EngineConfig,
    models_dir: &Path,
    ctx: &RunContext<'_>,
) -> Result<BackendKind, ModuleError> {
    if let Some(kind) = BackendKind::parse(setting) {
        return Ok(kind);
    }
    if setting != "auto" {
        return Err(ModuleError::InvalidSetting {
            key: "run.backend".into(),
            message: format!("unknown backend `{setting}` (use auto, llama-cpp or ollama)"),
        });
    }
    let has_gguf = !gguf_candidates(ctx, models_dir, &mut Vec::new()).is_empty();
    if config.llama_server().is_some() && has_gguf {
        return Ok(BackendKind::LlamaCpp);
    }
    if infermark_engine::ollama::version(&config.ollama_url).is_some() {
        return Ok(BackendKind::Ollama);
    }
    Err(ModuleError::Skipped(if has_gguf {
        "llama.cpp is not installed yet. Install it on the Models page.".into()
    } else {
        "no models downloaded yet. Download one on the Models page.".into()
    }))
}

/// Downloaded catalog models plus the profile's local `.gguf` files.
fn gguf_candidates(
    ctx: &RunContext<'_>,
    models_dir: &Path,
    notes: &mut Vec<String>,
) -> Vec<Candidate> {
    let sel = &ctx.profile.models;
    let catalog = Catalog::builtin();
    let store = ModelStore::new(models_dir);
    let mut out: Vec<Candidate> = catalog
        .models
        .iter()
        .filter(|m| sel.catalog.is_empty() || sel.catalog.contains(&m.id))
        .filter(|m| sel.max_params_b.map_or(true, |max| m.params_b <= max))
        .filter(|m| store.is_installed(m))
        .map(|m| Candidate {
            key: m.id.clone(),
            name: m.name.clone(),
            params_b: Some(m.params_b),
            spec: ModelSpec {
                id: m.id.clone(),
                location: store.path(m).to_string_lossy().into_owned(),
                sha256: Some(m.sha256.clone()),
            },
            gate_input: Some((m.size_bytes, m.shape)),
        })
        .collect();
    for raw in &sel.local_paths {
        let path = PathBuf::from(raw);
        let header = gguf::read_file(&path);
        let shape = header.as_ref().ok().and_then(|h| h.shape().ok());
        match (std::fs::metadata(&path), header) {
            (Ok(meta), Ok(h)) => {
                let stem = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                out.push(Candidate {
                    key: stem.to_lowercase(),
                    name: h.name().map(String::from).unwrap_or(stem),
                    params_b: None,
                    spec: ModelSpec {
                        id: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        location: raw.clone(),
                        sha256: None,
                    },
                    gate_input: shape.map(|s| (meta.len(), s)),
                });
            }
            (Err(e), _) => notes.push(format!("Skipped local model {}: {e}", file_label(&path))),
            (_, Err(e)) => notes.push(format!("Skipped local model {}: {e}", file_label(&path))),
        }
    }
    out
}

/// Just the file name: results never contain local folders.
fn file_label(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Profile `ollama` tags, plus catalog models whose Ollama tag is installed.
fn ollama_candidates(
    ctx: &RunContext<'_>,
    config: &EngineConfig,
    notes: &mut Vec<String>,
) -> Vec<Candidate> {
    let installed = match infermark_engine::ollama::list_models(&config.ollama_url) {
        Ok(m) => m,
        Err(e) => {
            notes.push(format!("Could not list Ollama models: {e}"));
            return vec![];
        }
    };
    let sel = &ctx.profile.models;
    let catalog = Catalog::builtin();
    let mut out = Vec::new();
    for m in &installed {
        let entry = catalog.models.iter().find(|c| {
            c.ollama.as_deref() == Some(m.name.as_str())
                || c.ollama.as_deref().map(|t| format!("{t}:latest")) == Some(m.name.clone())
        });
        let wanted = sel.ollama.contains(&m.name)
            || entry.is_some_and(|e| {
                (sel.catalog.is_empty() || sel.catalog.contains(&e.id))
                    && sel.max_params_b.map_or(true, |max| e.params_b <= max)
            });
        if !wanted {
            continue;
        }
        out.push(Candidate {
            key: m.name.replace(':', "-"),
            name: entry
                .map(|e| e.name.clone())
                .unwrap_or_else(|| m.name.clone()),
            params_b: entry.map(|e| e.params_b),
            spec: ModelSpec {
                id: m.name.clone(),
                location: m.name.clone(),
                sha256: None,
            },
            gate_input: entry.map(|e| (m.size_bytes.max(e.size_bytes / 2), e.shape)),
        });
    }
    for tag in &sel.ollama {
        if !installed.iter().any(|m| &m.name == tag) {
            notes.push(format!(
                "Skipped {tag}: not installed in Ollama (ollama pull {tag})."
            ));
        }
    }
    out
}

/// A prompt of roughly `tokens` tokens. Deterministic, except for the session number at the
/// very start, which defeats prompt caches.
fn perf_prompt(tokens: u32, session: u32) -> String {
    const TEXT: &str = "The harbour town woke slowly. Fishing boats rocked against the pier while \
        gulls argued over scraps, and the baker on the corner pulled the first loaves from a brick \
        oven older than anyone could remember. Up the hill, the lighthouse keeper logged the \
        weather in a notebook full of careful handwriting: wind from the west, light rain, \
        visibility fair. Children ran past the market stalls on their way to school, and the \
        ferry sounded its horn twice before leaving for the islands. ";
    let target_chars = (tokens as usize).saturating_sub(16) * 4;
    let mut prompt = format!("Session {session}. Read this passage.\n\n");
    while prompt.len() < target_chars {
        prompt.push_str(TEXT);
    }
    prompt.push_str("\n\nNow continue the story in your own words:\n");
    prompt
}

fn measure(
    backend: &mut dyn Backend,
    c: &Candidate,
    options: &LoadOptions,
    plan: Plan,
    ctx: &RunContext<'_>,
    report: &dyn Fn(f64, &str),
) -> Result<Vec<GenerationStats>, ModuleError> {
    report(0.0, "loading");
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
            report((w as f64 + 0.5) / (steps + 0.5), "warming up");
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
            );
            session += 1;
            let stats = backend
                .generate(
                    &request(plan.prompt_tokens, plan.gen_tokens, session),
                    &mut |_| {},
                )
                .map_err(ModuleError::Failed)?;
            samples.push(stats);
        }
        Ok(())
    })();
    backend.unload();
    result.map(|_| samples)
}

fn request(prompt_tokens: u32, gen_tokens: u32, session: u32) -> GenerateRequest {
    GenerateRequest {
        system: None,
        prompt: perf_prompt(prompt_tokens, session),
        max_tokens: gen_tokens,
        temperature: 0.0,
        stop: vec![],
        think: false,
        ignore_eos: true,
    }
}

fn push_results(out: &mut ModuleOutput, c: &Candidate, samples: &[GenerationStats]) {
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
        "{}: {prompt_tokens}-token prompt, {out_tokens} tokens generated, median of {} runs (generation spread {:.1}-{:.1} tok/s).",
        c.name,
        samples.len(),
        gen.iter().cloned().fold(f64::INFINITY, f64::min),
        gen.iter().cloned().fold(0.0, f64::max),
    ));
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 0 {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    }
}

fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

#[cfg(test)]
mod tests {
    use super::*;
    use infermark_sdk::{Profile, TokenEvent};
    use std::sync::atomic::AtomicBool;

    /// Pretends to be an engine: 100 tok/s prompt, 20 tok/s generation.
    struct FakeBackend {
        loaded: bool,
        calls: u32,
    }

    impl Backend for FakeBackend {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn version(&self) -> String {
            "1".into()
        }
        fn load(&mut self, _: &ModelSpec, _: &LoadOptions) -> Result<(), String> {
            self.loaded = true;
            Ok(())
        }
        fn generate(
            &mut self,
            req: &GenerateRequest,
            on: &mut dyn FnMut(TokenEvent),
        ) -> Result<GenerationStats, String> {
            assert!(self.loaded);
            self.calls += 1;
            on(TokenEvent {
                text: "x".into(),
                thinking: false,
            });
            let prompt_tokens = (req.prompt.len() / 4) as u32;
            Ok(GenerationStats {
                prompt_tokens,
                output_tokens: req.max_tokens,
                load_ms: 1500.0,
                prompt_eval_ms: prompt_tokens as f64 * 10.0,
                generation_ms: req.max_tokens as f64 * 50.0,
                time_to_first_token_ms: 300.0 + self.calls as f64,
                ..Default::default()
            })
        }
        fn unload(&mut self) {
            self.loaded = false;
        }
    }

    #[test]
    fn prompt_has_roughly_the_requested_length_and_varies_by_session() {
        let p = perf_prompt(512, 1);
        let approx_tokens = p.len() / 4;
        assert!((480..640).contains(&approx_tokens), "{approx_tokens}");
        assert_ne!(perf_prompt(512, 1), perf_prompt(512, 2));
        assert_eq!(perf_prompt(512, 3), perf_prompt(512, 3));
    }

    #[test]
    fn median_works() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn measures_and_reports_medians() {
        let hw = infermark_hwprobe_stub();
        let profile =
            Profile::from_toml("schema = 1\nname = \"t\"\n[run]\nrepeats = 3\nwarmups = 1\n")
                .unwrap();
        let settings = toml::Table::new();
        let cancel = AtomicBool::new(false);
        let ctx = RunContext {
            hardware: &hw,
            settings: &settings,
            run: &profile.run,
            profile: &profile,
            earlier: &[],
            progress: &|_| {},
            cancel: &cancel,
        };
        let c = Candidate {
            key: "fake-1b".into(),
            name: "Fake 1B".into(),
            params_b: Some(1.0),
            spec: ModelSpec {
                id: "fake".into(),
                location: "fake.gguf".into(),
                sha256: None,
            },
            gate_input: None,
        };
        let plan = Plan {
            prompt_tokens: 512,
            gen_tokens: 128,
            context: 1024,
            warmups: 1,
            repeats: 3,
        };
        let mut backend = FakeBackend {
            loaded: false,
            calls: 0,
        };
        let samples = measure(
            &mut backend,
            &c,
            &LoadOptions::default(),
            plan,
            &ctx,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(samples.len(), 3);
        assert_eq!(backend.calls, 4, "1 warm-up + 3 measured");
        assert!(!backend.loaded, "model is unloaded afterwards");

        let mut out = ModuleOutput::default();
        push_results(&mut out, &c, &samples);
        let get = |k: &str| out.metrics.iter().find(|m| m.key == k).unwrap().value;
        assert_eq!(get("fake-1b/gen_tps"), 20.0);
        assert_eq!(get("fake-1b/prompt_tps"), 100.0);
        assert_eq!(get("fake-1b/ttft_ms"), 303.0);
        assert_eq!(get("fake-1b/load_s"), 1.5);
        assert_eq!(out.series[0].points.len(), 3);
    }

    #[test]
    fn cancel_stops_between_runs() {
        let hw = infermark_hwprobe_stub();
        let profile = Profile::from_toml("schema = 1\nname = \"t\"\n").unwrap();
        let settings = toml::Table::new();
        let cancel = AtomicBool::new(true);
        let ctx = RunContext {
            hardware: &hw,
            settings: &settings,
            run: &profile.run,
            profile: &profile,
            earlier: &[],
            progress: &|_| {},
            cancel: &cancel,
        };
        let c = Candidate {
            key: "k".into(),
            name: "n".into(),
            params_b: None,
            spec: ModelSpec {
                id: "x".into(),
                location: "x".into(),
                sha256: None,
            },
            gate_input: None,
        };
        let plan = Plan {
            prompt_tokens: 64,
            gen_tokens: 8,
            context: 1024,
            warmups: 1,
            repeats: 2,
        };
        let mut backend = FakeBackend {
            loaded: false,
            calls: 0,
        };
        let err = measure(
            &mut backend,
            &c,
            &LoadOptions::default(),
            plan,
            &ctx,
            &|_, _| {},
        )
        .unwrap_err();
        assert!(matches!(err, ModuleError::Cancelled));
        assert_eq!(backend.calls, 0);
        assert!(!backend.loaded);
    }

    fn infermark_hwprobe_stub() -> infermark_sdk::HardwareInfo {
        serde_json::from_value(serde_json::json!({
            "fingerprint": "t",
            "os": {"family": "windows", "name": "W", "version": "11", "kernel": "", "arch": "x86_64"},
            "cpu": {"brand": "c", "vendor": "", "physical_cores": 4, "logical_cores": 8, "frequency_mhz": null},
            "memory": {"total_bytes": 17179869184u64, "available_bytes": 8589934592u64},
            "gpus": [], "npus": [], "battery": null, "power_source": "ac"
        }))
        .unwrap()
    }
}
