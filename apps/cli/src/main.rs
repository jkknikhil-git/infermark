//! `infermark` — the command-line runner. Uses the same core as the desktop app, so a profile
//! gives the same results in both.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use infermark_core::engine::{self, EngineCatalog, EngineStore};
use infermark_core::models::download::{DownloadPolicy, Progress};
use infermark_core::models::{Catalog, ModelStore};
use infermark_core::{
    model_list, packs, paths, plan, profiles, run, schema, settings, Registry, RunEvent,
};
use infermark_sdk::{GateSettings, HardwareInfo, ModuleStatus, PlanStatus, PowerSource, RunResult};

#[derive(Parser)]
#[command(
    name = "infermark",
    version,
    about = "The open-source benchmark for local AI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the detected hardware.
    Probe {
        /// Print JSON instead of a summary.
        #[arg(long)]
        json: bool,
    },
    /// List built-in and user profiles.
    Profiles {
        #[arg(long)]
        json: bool,
    },
    /// Check a profile file for mistakes.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// List the modules in this build and what they require.
    Modules {
        #[arg(long)]
        json: bool,
    },
    /// Show what a profile would run on this machine, and what gets skipped (and why).
    Plan {
        /// Profile name (`quick`) or path to a .toml file.
        #[arg(long, short, default_value = "quick")]
        profile: String,
        #[arg(long)]
        json: bool,
    },
    /// Run a profile and save the result.
    Run {
        /// Profile name (`quick`) or path to a .toml file.
        #[arg(long, short, default_value = "quick")]
        profile: String,
        /// Folder for the result file (default: ~/.infermark/results).
        #[arg(long, short)]
        out: Option<PathBuf>,
        /// Print the full result JSON to stdout when done.
        #[arg(long)]
        json: bool,
        /// Don't print progress.
        #[arg(long, short)]
        quiet: bool,
    },
    /// List installed test packs.
    Packs {
        #[arg(long)]
        json: bool,
    },
    /// Work with a single test pack.
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },
    /// JSON Schemas for result, profile and pack files.
    Schema {
        #[command(subcommand)]
        action: SchemaAction,
    },
    /// List catalog models and whether each fits this device; download or delete them.
    Models {
        #[command(subcommand)]
        action: Option<ModelsAction>,
        #[arg(long)]
        json: bool,
    },
    /// Show the inference engines (llama.cpp, Ollama); install llama.cpp.
    Engine {
        #[command(subcommand)]
        action: Option<EngineAction>,
        #[arg(long)]
        json: bool,
    },
    /// Show or change settings (where models are stored, Ollama address, ...).
    Settings {
        #[command(subcommand)]
        action: Option<SettingsAction>,
    },
}

#[derive(Subcommand)]
enum ModelsAction {
    /// Download a catalog model (resumes if interrupted; verified with SHA-256).
    Pull { id: String },
    /// Delete a downloaded model.
    Rm { id: String },
}

#[derive(Subcommand)]
enum EngineAction {
    /// Download and install the llama.cpp build recommended for this device.
    Install {
        /// Pick a specific build: cpu, vulkan, cuda or metal.
        #[arg(long)]
        accel: Option<String>,
    },
    /// Remove installed llama.cpp builds.
    Rm,
}

#[derive(Subcommand)]
enum SettingsAction {
    /// Change a setting; an empty value resets it. Keys: storage_dir, llama_server, ollama_url.
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum ProfileAction {
    /// Validate a profile .toml file.
    Validate { path: PathBuf },
}

#[derive(Subcommand)]
enum PackAction {
    /// Validate a pack folder (pack.toml + items.jsonl).
    Validate { dir: PathBuf },
}

#[derive(Subcommand)]
enum SchemaAction {
    /// Write the schemas as JSON files.
    Export {
        #[arg(long, default_value = "schema")]
        dir: PathBuf,
    },
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<ExitCode> {
    let registry = Registry::builtin();
    match cli.command {
        Command::Probe { json } => {
            let hw = infermark_hwprobe::probe();
            if json {
                println!("{}", serde_json::to_string_pretty(&hw)?);
            } else {
                print_hardware(&hw);
            }
        }
        Command::Profiles { json } => {
            let (entries, errors) = profiles::list();
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for e in &entries {
                    let source = match &e.source {
                        profiles::ProfileSource::Builtin => "built-in".to_string(),
                        profiles::ProfileSource::User(p) => p.display().to_string(),
                    };
                    println!("{:<16} {:<18} {}", e.key, e.profile.name, source);
                    if !e.profile.description.is_empty() {
                        println!("{:<16} {}", "", e.profile.description);
                    }
                }
            }
            for (path, err) in &errors {
                eprintln!("warning: skipped {}: {err}", path.display());
            }
        }
        Command::Profile {
            action: ProfileAction::Validate { path },
        } => {
            let p = profiles::load_file(&path)?;
            println!(
                "OK: {} ({} enabled modules)",
                p.name,
                p.enabled_modules().count()
            );
        }
        Command::Modules { json } => {
            let descriptors: Vec<_> = registry.modules().map(|m| m.descriptor()).collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&descriptors)?);
            } else {
                for d in descriptors {
                    println!("{:<20} {}", d.id, d.description);
                    for r in &d.requirements {
                        println!("{:<20}   requires {}", "", serde_json::to_string(r)?);
                    }
                }
            }
        }
        Command::Plan { profile, json } => {
            let p = profiles::resolve(&profile)?;
            let hw = infermark_hwprobe::probe();
            let entries = plan(&p, &registry, &hw);
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                println!("Profile: {}", p.name);
                for e in &entries {
                    match &e.status {
                        PlanStatus::Planned => println!("  run   {}", e.module_id),
                        PlanStatus::Skipped { reason } => {
                            println!("  skip  {} ({reason})", e.module_id)
                        }
                    }
                }
            }
        }
        Command::Run {
            profile,
            out,
            json,
            quiet,
        } => {
            let p = profiles::resolve(&profile)?;
            let hw = infermark_hwprobe::probe();
            let on_event = |event: RunEvent| {
                if !quiet {
                    print_event(&event);
                }
            };
            let result = run(&p, &registry, &hw, &on_event, &AtomicBool::new(false));
            let path = save_result(&result, out.as_deref().unwrap_or(&paths::results_dir()))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_summary(&result);
            }
            eprintln!("Saved {}", path.display());
            let failed = result
                .modules
                .iter()
                .any(|m| matches!(m.status, ModuleStatus::Failed { .. }));
            if failed {
                return Ok(ExitCode::from(2));
            }
        }
        Command::Packs { json } => {
            let (found, errors) = packs::discover(&paths::pack_search_dirs(), &registry);
            if json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else if found.is_empty() {
                println!(
                    "No packs installed. Put pack folders in {}",
                    paths::user_packs_dir().display()
                );
            } else {
                for pack in &found {
                    let m = &pack.manifest;
                    println!(
                        "{:<24} {:<16} {:>4} items  {}",
                        m.id,
                        m.category,
                        pack.items.len(),
                        m.name
                    );
                }
            }
            for e in &errors {
                eprintln!("warning: {e}");
            }
        }
        Command::Pack {
            action: PackAction::Validate { dir },
        } => match packs::load(&dir, &registry) {
            Ok(pack) => println!(
                "OK: {} v{} ({} items, scorer {})",
                pack.manifest.id,
                pack.manifest.version,
                pack.items.len(),
                pack.manifest.scorer
            ),
            Err(e) => {
                eprintln!("{e}");
                return Ok(ExitCode::FAILURE);
            }
        },
        Command::Schema {
            action: SchemaAction::Export { dir },
        } => {
            std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
            for (name, text) in schema::all() {
                let path = dir.join(name);
                std::fs::write(&path, text)
                    .with_context(|| format!("writing {}", path.display()))?;
                println!("wrote {}", path.display());
            }
        }
        Command::Models { action, json } => models_command(action, json)?,
        Command::Engine { action, json } => engine_command(action, json)?,
        Command::Settings { action } => {
            let mut s = settings::load()?;
            if let Some(SettingsAction::Set { key, value }) = action {
                s.set(&key, &value)?;
                settings::save(&s)?;
                println!("Saved {}", settings::path().display());
            }
            println!("storage_dir   {}", s.storage_dir().display());
            println!(
                "llama_server  {}",
                s.llama_server
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(installed by InferMark)".into())
            );
            println!("ollama_url    {}", s.ollama_url);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn models_command(action: Option<ModelsAction>, json: bool) -> Result<()> {
    let s = settings::load()?;
    let store = ModelStore::new(s.models_dir());
    let catalog = Catalog::builtin();
    let find = |id: &str| {
        catalog
            .get(id)
            .with_context(|| format!("no model `{id}` in the catalog (see `infermark models`)"))
    };
    match action {
        None => {
            let hw = infermark_hwprobe::probe();
            let bandwidth = model_list::latest_bandwidth(&hw.fingerprint);
            let entries = model_list::list(&hw, &GateSettings::default(), bandwidth, &store);
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }
            println!("Models folder: {}", store.root().display());
            if bandwidth.is_none() {
                println!("(Run a benchmark once so speed can be predicted.)");
            }
            for e in &entries {
                let state = if e.installed { "installed" } else { "" };
                let speed = e
                    .verdict
                    .predicted_tokens_per_sec
                    .map(|t| format!("~{t:.0} tok/s"))
                    .unwrap_or_default();
                println!(
                    "{:<26} {:>6.1} GB  {:<16} {:<10} {:>10}  {}",
                    e.model.id,
                    e.model.size_bytes as f64 / 1e9,
                    e.verdict.tier.label(),
                    state,
                    speed,
                    e.verdict.summary
                );
            }
        }
        Some(ModelsAction::Pull { id }) => {
            let model = find(&id)?;
            println!(
                "{} ({}, {:.2} GB, license: {})",
                model.name,
                model.quant,
                model.size_bytes as f64 / 1e9,
                model.license
            );
            println!("Saving to {}", store.path(model).display());
            let path = store.install(
                model,
                &DownloadPolicy::default(),
                &mut print_progress,
                &AtomicBool::new(false),
            )?;
            eprintln!();
            println!("Verified and installed: {}", path.display());
        }
        Some(ModelsAction::Rm { id }) => {
            store.remove(find(&id)?)?;
            println!("Deleted {id}");
        }
    }
    Ok(())
}

fn engine_command(action: Option<EngineAction>, json: bool) -> Result<()> {
    let s = settings::load()?;
    let catalog = EngineCatalog::builtin();
    let store = EngineStore::new(s.engines_dir());
    match action {
        None => {
            let status = engine::status(&s.engine_config());
            if json {
                println!("{}", serde_json::to_string_pretty(&status)?);
                return Ok(());
            }
            match &status.llama_cpp {
                Some(l) => println!(
                    "llama.cpp  {} {} ({})",
                    l.version,
                    l.accel.map(|a| a.label()).unwrap_or("custom build"),
                    l.path.display()
                ),
                None => println!("llama.cpp  not installed (run `infermark engine install`)"),
            }
            match &status.ollama {
                Some(o) => println!(
                    "Ollama     {} at {} ({} models)",
                    o.version,
                    o.url,
                    o.models.len()
                ),
                None => println!("Ollama     not running at {}", s.ollama_url),
            }
        }
        Some(EngineAction::Install { accel }) => {
            let builds = catalog.for_this_platform();
            let build = match accel {
                Some(a) => builds
                    .into_iter()
                    .find(|b| b.accel.id() == a)
                    .with_context(|| format!("no `{a}` build of llama.cpp for this platform"))?,
                None => match catalog.recommended(&infermark_hwprobe::probe()) {
                    Some(b) => b,
                    None => bail!("no llama.cpp build for this platform"),
                },
            };
            println!(
                "llama.cpp {} for {} ({:.0} MB download)",
                build.build,
                build.accel.label(),
                build.download_bytes() as f64 / 1e6
            );
            let path = store.install(
                build,
                &DownloadPolicy::default(),
                &mut |i, n, p| {
                    if n > 1 {
                        eprint!("[{}/{}] ", i + 1, n);
                    }
                    print_progress(p)
                },
                &AtomicBool::new(false),
            )?;
            eprintln!();
            println!("Installed {}", path.display());
        }
        Some(EngineAction::Rm) => {
            for b in catalog.for_this_platform() {
                store.remove(b)?;
            }
            println!("Removed installed llama.cpp builds");
        }
    }
    Ok(())
}

fn print_progress(p: Progress) {
    let (what, done, total) = match p {
        Progress::Downloading { done, total } => ("Downloading", done, total),
        Progress::Verifying { done, total } => ("Verifying  ", done, total),
    };
    eprint!(
        "\r{what} {:>5.1}%  {:.2} / {:.2} GB",
        done as f64 * 100.0 / total.max(1) as f64,
        done as f64 / 1e9,
        total as f64 / 1e9
    );
}

fn save_result(result: &RunResult, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = format!(
        "{}-{}.json",
        result.started_at.format("%Y%m%d-%H%M%S"),
        result.hardware.fingerprint
    );
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(result)?)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

fn print_hardware(hw: &HardwareInfo) {
    println!("OS       {} {} ({})", hw.os.name, hw.os.version, hw.os.arch);
    let cores = hw
        .cpu
        .physical_cores
        .map(|p| format!("{p} cores / {} threads", hw.cpu.logical_cores))
        .unwrap_or_else(|| format!("{} threads", hw.cpu.logical_cores));
    println!("CPU      {} ({cores})", hw.cpu.brand);
    println!(
        "Memory   {:.1} GB ({:.1} GB free)",
        hw.total_ram_gb(),
        hw.memory.available_bytes as f64 / GIB
    );
    if hw.gpus.is_empty() {
        println!("GPU      none detected");
    }
    for g in &hw.gpus {
        let mem = match (g.integrated, g.dedicated_vram_bytes) {
            (true, _) => "integrated".to_string(),
            (false, Some(v)) => format!("{:.1} GB", v as f64 / GIB),
            (false, None) => "memory unknown".to_string(),
        };
        let driver = g
            .driver_version
            .as_deref()
            .map(|d| format!(", driver {d}"))
            .unwrap_or_default();
        println!("GPU      {} ({mem}{driver})", g.name);
    }
    for n in &hw.npus {
        println!("NPU      {}", n.name);
    }
    match &hw.battery {
        Some(b) => println!(
            "Battery  {:.0}%{}",
            b.charge_percent,
            if b.charging { " (charging)" } else { "" }
        ),
        None => println!("Battery  none"),
    }
    let power = match hw.power_source {
        PowerSource::Ac => "Plugged in",
        PowerSource::Battery => "On battery",
        PowerSource::Unknown => "Unknown",
    };
    println!("Power    {power}");
    println!("ID       {}", hw.fingerprint);
}

fn print_event(event: &RunEvent) {
    match event {
        RunEvent::ModuleStarted {
            module_id,
            index,
            total,
        } => eprintln!("[{}/{}] {module_id}", index + 1, total),
        RunEvent::ModuleProgress { progress, .. } => match &progress.live {
            Some(live) => eprintln!(
                "       {:>3.0}%  {}  ({} {})",
                progress.fraction * 100.0,
                progress.message,
                live.value,
                live.unit
            ),
            None => eprintln!(
                "       {:>3.0}%  {}",
                progress.fraction * 100.0,
                progress.message
            ),
        },
        // Sensors are saved in the result; the terminal only shows progress.
        RunEvent::RunStarted { .. } | RunEvent::Sensors { .. } => {}
        RunEvent::ModuleFinished { record } => match &record.status {
            ModuleStatus::Completed => {
                eprintln!("       done in {:.1}s", record.duration_ms / 1000.0)
            }
            ModuleStatus::Skipped { reason } => eprintln!("       skipped: {reason}"),
            ModuleStatus::Failed { error } => eprintln!("       FAILED: {error}"),
            ModuleStatus::Cancelled => eprintln!("       cancelled"),
        },
    }
}

fn print_summary(result: &RunResult) {
    println!();
    println!("Results ({})", result.profile.name);
    for m in &result.modules {
        if m.status != ModuleStatus::Completed {
            continue;
        }
        println!("  {}", m.module_id);
        for metric in &m.output.metrics {
            println!("    {:<24} {} {}", metric.label, metric.value, metric.unit);
        }
        for note in &m.output.notes {
            println!("    note: {note}");
        }
    }
}
