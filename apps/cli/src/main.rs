//! `infermark` — the command-line runner. Uses the same core as the desktop app, so a profile
//! gives the same results in both.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use infermark_core::{packs, paths, plan, profiles, run, schema, Registry, RunEvent};
use infermark_sdk::{HardwareInfo, ModuleStatus, PlanStatus, PowerSource, RunResult};

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
    }
    Ok(ExitCode::SUCCESS)
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
        RunEvent::ModuleProgress { progress, .. } => {
            eprintln!(
                "       {:>3.0}%  {}",
                progress.fraction * 100.0,
                progress.message
            )
        }
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
