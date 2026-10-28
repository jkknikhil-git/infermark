//! What every model-running module shares: picking the backend, listing the models this device
//! can run (through the hardware gate), the prompt, and a timed generation loop.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use infermark_engine::{open_backend, BackendKind, EngineConfig};
use infermark_models::{assess, gguf, Catalog, ModelShape, ModelStore, Verdict};
use infermark_sdk::{Backend, GenerateRequest, LoadOptions, ModelSpec, ModuleError, RunContext};

use crate::settings;

/// A model this run could benchmark.
#[derive(Debug, Clone)]
pub(super) struct Candidate {
    /// Catalog id, file stem or Ollama tag; used as the metric key prefix.
    pub key: String,
    pub name: String,
    pub params_b: Option<f64>,
    pub spec: ModelSpec,
    /// File size and shape, when known, for the hardware gate.
    pub gate_input: Option<(u64, ModelShape)>,
}

/// The backend to use and the models to run on it, smallest first.
pub(super) struct Selection {
    pub kind: BackendKind,
    pub config: EngineConfig,
    pub candidates: Vec<Candidate>,
}

/// Models that pass the hardware gate for `context` tokens, smallest first, at most `max`.
/// Skipped and left-out models are explained in `notes`.
pub(super) fn select_models(
    ctx: &RunContext<'_>,
    max: usize,
    context: u32,
    notes: &mut Vec<String>,
) -> Result<Selection, ModuleError> {
    let settings = settings::load_or_default();
    let config = settings.engine_config();
    let kind = choose_backend(&ctx.run.backend, &config, &settings.models_dir(), ctx)?;
    let mut candidates = match kind {
        BackendKind::LlamaCpp | BackendKind::LlamaCppInProcess => {
            gguf_candidates(ctx, &settings.models_dir(), notes)
        }
        BackendKind::Ollama => ollama_candidates(ctx, &config, notes),
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
            context,
            ctx.hardware,
            &ctx.profile.gate,
            bandwidth,
        );
        if v.runnable(&ctx.profile.gate) {
            true
        } else {
            notes.push(format!("Skipped {}: {}.", c.name, v.summary));
            false
        }
    });
    candidates.sort_by(|a, b| {
        a.params_b
            .unwrap_or(f64::MAX)
            .total_cmp(&b.params_b.unwrap_or(f64::MAX))
    });
    if candidates.len() > max {
        let rest: Vec<_> = candidates.drain(max..).map(|c| c.name).collect();
        notes.push(format!(
            "Tested the {max} smallest model(s); not tested this time: {}.",
            rest.join(", ")
        ));
    }
    if candidates.is_empty() {
        return Err(ModuleError::Skipped(match kind {
            BackendKind::LlamaCpp | BackendKind::LlamaCppInProcess => {
                "no downloaded models fit this profile. Download one on the Models page.".into()
            }
            BackendKind::Ollama => "no Ollama models fit this profile".into(),
        }));
    }
    Ok(Selection {
        kind,
        config,
        candidates,
    })
}

/// Open the selected backend; the engine and its version become the first note.
pub(super) fn open(
    sel: &Selection,
    notes: &mut Vec<String>,
) -> Result<Box<dyn Backend>, ModuleError> {
    let backend = open_backend(sel.kind, &sel.config).map_err(ModuleError::Failed)?;
    notes.insert(
        0,
        format!("Engine: {} {}", sel.kind.label(), backend.version()),
    );
    Ok(backend)
}

/// Context size that fits a prompt plus its answer with some room to spare.
pub(super) fn context_for(prompt_tokens: u32, gen_tokens: u32) -> u32 {
    (prompt_tokens + gen_tokens + 128)
        .next_power_of_two()
        .max(1024)
}

/// Load options for this profile.
pub(super) fn load_options(ctx: &RunContext<'_>, context: u32) -> LoadOptions {
    LoadOptions {
        context_size: context,
        threads: ctx.run.threads,
        gpu_layers: ctx.run.gpu_layers,
        seed: ctx.run.seed,
    }
}

/// One generation in a [`generation_loop`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Loop {
    /// Seconds since [`generation_loop`] was called (loading included) when this generation
    /// finished, so it lines up with the module's start.
    pub t: f64,
    pub tokens_per_sec: f64,
    pub tokens: u32,
}

/// What a [`generation_loop`] runs.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopPlan {
    pub prompt_tokens: u32,
    pub gen_tokens: u32,
    pub duration: Duration,
}

/// Load `c`, then generate back to back until `plan.duration` has passed (the last generation
/// is allowed to finish). `on_loop` sees every generation as it completes, for progress and for
/// reading sensors, and can stop the loop with an error. Stops between generations when the
/// run is cancelled.
pub(super) fn generation_loop(
    backend: &mut dyn Backend,
    c: &Candidate,
    options: &LoadOptions,
    plan: LoopPlan,
    ctx: &RunContext<'_>,
    on_loop: &mut dyn FnMut(&Loop) -> Result<(), ModuleError>,
) -> Result<Vec<Loop>, ModuleError> {
    let origin = Instant::now();
    backend
        .load(&c.spec, options)
        .map_err(ModuleError::Failed)?;
    let result = (|| {
        // One short warm-up so the first loop doesn't pay for loading and shader compilation.
        backend
            .generate(&request(plan.prompt_tokens.min(128), 16, 0), &mut |_| {})
            .map_err(ModuleError::Failed)?;
        let start = Instant::now();
        let mut loops = Vec::new();
        let mut session = 0;
        while start.elapsed() < plan.duration {
            if ctx.is_cancelled() {
                return Err(ModuleError::Cancelled);
            }
            session += 1;
            let stats = backend
                .generate(
                    &request(plan.prompt_tokens, plan.gen_tokens, session),
                    &mut |_| {},
                )
                .map_err(ModuleError::Failed)?;
            let l = Loop {
                t: round(origin.elapsed().as_secs_f64(), 2),
                tokens_per_sec: round(stats.generation_tokens_per_sec(), 2),
                tokens: stats.output_tokens,
            };
            on_loop(&l)?;
            loops.push(l);
        }
        Ok(loops)
    })();
    backend.unload();
    result
}

/// `minutes` from the module settings (decimals allowed), within 15 s and 2 h.
pub(super) fn duration_setting(ctx: &RunContext<'_>, default_minutes: f64) -> Duration {
    let minutes = ctx.setting_f64("minutes", default_minutes);
    let minutes = if minutes.is_finite() {
        minutes
    } else {
        default_minutes
    };
    Duration::from_secs_f64(minutes.clamp(0.25, 120.0) * 60.0)
}

/// "45 s", "2.5 min".
pub(super) fn minutes_label(seconds: f64) -> String {
    if seconds < 90.0 {
        format!("{seconds:.0} s")
    } else {
        format!("{:.1} min", seconds / 60.0)
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
            message: format!(
                "unknown backend `{setting}` (use auto, llama-cpp, llama-cpp-inproc or ollama)"
            ),
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
pub(super) fn perf_prompt(tokens: u32, session: u32) -> String {
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

pub(super) fn request(prompt_tokens: u32, gen_tokens: u32, session: u32) -> GenerateRequest {
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

pub(super) fn median(values: &[f64]) -> f64 {
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

pub(super) fn round(v: f64, digits: i32) -> f64 {
    let p = 10f64.powi(digits);
    (v * p).round() / p
}

#[cfg(test)]
pub(super) mod testing {
    use std::sync::atomic::AtomicBool;

    use infermark_sdk::{
        Backend, GenerateRequest, GenerationStats, HardwareInfo, LoadOptions, ModelSpec, Profile,
        RunContext, SensorSnapshot, TokenEvent,
    };

    /// Pretends to be an engine: 100 tok/s prompt; generation speed from `speed(call)`, and
    /// each generation takes `ms` of real time.
    pub struct FakeBackend {
        pub loaded: bool,
        pub calls: u32,
        pub speed: fn(u32) -> f64,
        pub ms: u64,
    }

    impl FakeBackend {
        pub fn new(speed: fn(u32) -> f64) -> Self {
            Self {
                loaded: false,
                calls: 0,
                speed,
                ms: 0,
            }
        }
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
            std::thread::sleep(std::time::Duration::from_millis(self.ms));
            on(TokenEvent {
                text: "x".into(),
                thinking: false,
            });
            let prompt_tokens = (req.prompt.len() / 4) as u32;
            let tps = (self.speed)(self.calls);
            Ok(GenerationStats {
                prompt_tokens,
                output_tokens: req.max_tokens,
                load_ms: 1500.0,
                prompt_eval_ms: prompt_tokens as f64 * 10.0,
                generation_ms: req.max_tokens as f64 * 1000.0 / tps,
                time_to_first_token_ms: 300.0 + self.calls as f64,
                ..Default::default()
            })
        }
        fn unload(&mut self) {
            self.loaded = false;
        }
    }

    pub fn candidate() -> super::Candidate {
        super::Candidate {
            key: "fake-1b".into(),
            name: "Fake 1B".into(),
            params_b: Some(1.0),
            spec: ModelSpec {
                id: "fake".into(),
                location: "fake.gguf".into(),
                sha256: None,
            },
            gate_input: None,
        }
    }

    /// A 16 GB laptop running on battery.
    pub fn hardware() -> HardwareInfo {
        serde_json::from_value(serde_json::json!({
            "fingerprint": "t",
            "os": {"family": "windows", "name": "W", "version": "11", "kernel": "", "arch": "x86_64"},
            "cpu": {"brand": "c", "vendor": "", "physical_cores": 4, "logical_cores": 8, "frequency_mhz": null},
            "memory": {"total_bytes": 17179869184u64, "available_bytes": 8589934592u64},
            "gpus": [], "npus": [],
            "battery": {"charge_percent": 80.0, "design_capacity_wh": 50.0, "full_capacity_wh": 48.0, "charging": false},
            "power_source": "battery"
        }))
        .unwrap()
    }

    /// Everything a [`RunContext`] borrows, owned by the test.
    pub struct Harness {
        pub hw: HardwareInfo,
        pub profile: Profile,
        pub settings: toml::Table,
        pub cancel: AtomicBool,
        pub sensors: SensorSnapshot,
    }

    impl Harness {
        pub fn new(profile_toml: &str) -> Self {
            Self {
                hw: hardware(),
                profile: Profile::from_toml(&format!("schema = 1\nname = \"t\"\n{profile_toml}"))
                    .unwrap(),
                settings: toml::Table::new(),
                cancel: AtomicBool::new(false),
                sensors: SensorSnapshot::default(),
            }
        }

        pub fn ctx(&self) -> RunContext<'_> {
            RunContext {
                hardware: &self.hw,
                settings: &self.settings,
                run: &self.profile.run,
                profile: &self.profile,
                earlier: &[],
                progress: &|_| {},
                cancel: &self.cancel,
                sensors: &self.sensors,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

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
    fn loop_runs_for_the_duration_then_unloads() {
        let h = Harness::new("");
        let mut backend = FakeBackend::new(|_| 40.0);
        backend.ms = 5;
        let plan = LoopPlan {
            prompt_tokens: 64,
            gen_tokens: 8,
            duration: Duration::from_millis(60),
        };
        let mut seen = 0;
        let loops = generation_loop(
            &mut backend,
            &candidate(),
            &LoadOptions::default(),
            plan,
            &h.ctx(),
            &mut |_| {
                seen += 1;
                Ok(())
            },
        )
        .unwrap();
        assert!(loops.len() >= 3, "{}", loops.len());
        assert_eq!(seen, loops.len());
        assert_eq!(backend.calls as usize, loops.len() + 1, "plus one warm-up");
        assert!(loops.windows(2).all(|w| w[0].t <= w[1].t));
        assert!(!backend.loaded);
    }

    #[test]
    fn loop_stops_when_cancelled_and_still_unloads() {
        let h = Harness::new("");
        h.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let mut backend = FakeBackend::new(|_| 40.0);
        let plan = LoopPlan {
            prompt_tokens: 64,
            gen_tokens: 8,
            duration: Duration::from_secs(60),
        };
        let err = generation_loop(
            &mut backend,
            &candidate(),
            &LoadOptions::default(),
            plan,
            &h.ctx(),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(err, ModuleError::Cancelled));
        assert!(!backend.loaded);
    }

    #[test]
    fn minutes_setting_is_clamped() {
        let mut h = Harness::new("");
        assert_eq!(duration_setting(&h.ctx(), 5.0), Duration::from_secs(300));
        h.settings
            .insert("minutes".into(), toml::Value::Float(0.01));
        assert_eq!(duration_setting(&h.ctx(), 5.0), Duration::from_secs(15));
        h.settings
            .insert("minutes".into(), toml::Value::Integer(1000));
        assert_eq!(duration_setting(&h.ctx(), 5.0), Duration::from_secs(7200));
    }
}
