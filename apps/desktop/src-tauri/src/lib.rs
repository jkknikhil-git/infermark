//! Tauri commands: a thin layer over `infermark-core`, so the app and the CLI behave the same.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use infermark_core::{packs, paths, plan, profiles, run as run_benchmark, Registry, RunEvent};
use infermark_sdk::{HardwareInfo, ModuleStatus, PlanEntry, RunResult};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    /// Cancel flag of the run in progress, if any.
    running: Mutex<Option<Arc<AtomicBool>>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[tauri::command]
async fn probe() -> HardwareInfo {
    tauri::async_runtime::spawn_blocking(infermark_hwprobe::probe)
        .await
        .expect("probe never panics")
}

#[derive(Serialize)]
struct ProfileList {
    profiles: Vec<profiles::ProfileEntry>,
    errors: Vec<(PathBuf, String)>,
}

#[tauri::command]
fn list_profiles() -> ProfileList {
    let (profiles, errors) = profiles::list();
    ProfileList { profiles, errors }
}

#[tauri::command]
async fn plan_profile(key: String) -> CmdResult<Vec<PlanEntry>> {
    tauri::async_runtime::spawn_blocking(move || {
        let profile = profiles::resolve(&key).map_err(err)?;
        let hw = infermark_hwprobe::probe();
        Ok(plan(&profile, &Registry::builtin(), &hw))
    })
    .await
    .map_err(err)?
}

/// Start a run in the background. Progress arrives as `run-event` events and the final result
/// as `run-finished` (or `run-error`).
#[tauri::command]
fn start_run(key: String, app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let profile = profiles::resolve(&key).map_err(err)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = state.running.lock().unwrap();
        if running.is_some() {
            return Err("A benchmark is already running.".into());
        }
        *running = Some(cancel.clone());
    }

    std::thread::spawn(move || {
        let hw = infermark_hwprobe::probe();
        let emitter = app.clone();
        let on_event = move |e: RunEvent| {
            let _ = emitter.emit("run-event", &e);
        };
        let result = run_benchmark(&profile, &Registry::builtin(), &hw, &on_event, &cancel);
        match save_result(&result) {
            Ok(path) => {
                let _ = app.emit("run-finished", summarize(&path, &result));
            }
            Err(e) => {
                let _ = app.emit("run-error", e);
            }
        }
        if let Some(state) = app.try_state::<AppState>() {
            *state.running.lock().unwrap() = None;
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_run(state: State<'_, AppState>) {
    if let Some(flag) = state.running.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
}

fn save_result(result: &RunResult) -> CmdResult<PathBuf> {
    let dir = paths::results_dir();
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(format!(
        "{}-{}.json",
        result.started_at.format("%Y%m%d-%H%M%S"),
        result.hardware.fingerprint
    ));
    let json = serde_json::to_string_pretty(result).map_err(err)?;
    std::fs::write(&path, json).map_err(err)?;
    Ok(path)
}

#[derive(Serialize, Clone)]
struct ResultSummary {
    path: PathBuf,
    run_id: String,
    profile_name: String,
    started_at: DateTime<Utc>,
    duration_ms: i64,
    completed: usize,
    skipped: usize,
    failed: usize,
}

fn summarize(path: &Path, r: &RunResult) -> ResultSummary {
    let count = |f: fn(&ModuleStatus) -> bool| r.modules.iter().filter(|m| f(&m.status)).count();
    ResultSummary {
        path: path.to_path_buf(),
        run_id: r.run_id.to_string(),
        profile_name: r.profile.name.clone(),
        started_at: r.started_at,
        duration_ms: (r.finished_at - r.started_at).num_milliseconds(),
        completed: count(|s| matches!(s, ModuleStatus::Completed)),
        skipped: count(|s| matches!(s, ModuleStatus::Skipped { .. })),
        failed: count(|s| matches!(s, ModuleStatus::Failed { .. })),
    }
}

/// Saved results, newest first. Unreadable files are skipped.
#[tauri::command]
fn list_results() -> Vec<ResultSummary> {
    let Ok(read) = std::fs::read_dir(paths::results_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<ResultSummary> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let r: RunResult = serde_json::from_str(&text).ok()?;
            Some(summarize(&p, &r))
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.started_at));
    out
}

#[tauri::command]
fn load_result(path: PathBuf) -> CmdResult<RunResult> {
    // Only files inside the results folder may be opened from the UI.
    let dir = paths::results_dir().canonicalize().map_err(err)?;
    let file = path.canonicalize().map_err(err)?;
    if !file.starts_with(&dir) {
        return Err("Not a result file.".into());
    }
    let text = std::fs::read_to_string(&file).map_err(err)?;
    serde_json::from_str(&text).map_err(err)
}

#[derive(Serialize)]
struct PackSummary {
    id: String,
    name: String,
    category: String,
    version: String,
    description: String,
    items: usize,
    scorer: String,
    subjective: bool,
    dir: PathBuf,
}

#[derive(Serialize)]
struct PackList {
    packs: Vec<PackSummary>,
    errors: Vec<String>,
}

#[tauri::command]
fn list_packs() -> PackList {
    let (found, errors) = packs::discover(&paths::pack_search_dirs(), &Registry::builtin());
    PackList {
        packs: found
            .into_iter()
            .map(|p| PackSummary {
                id: p.manifest.id,
                name: p.manifest.name,
                category: p.manifest.category,
                version: p.manifest.version,
                description: p.manifest.description,
                items: p.items.len(),
                scorer: p.manifest.scorer,
                subjective: p.manifest.subjective,
                dir: p.dir,
            })
            .collect(),
        errors: errors.into_iter().map(|e| e.to_string()).collect(),
    }
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    data_dir: PathBuf,
    profiles_dir: PathBuf,
    packs_dir: PathBuf,
    results_dir: PathBuf,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: infermark_core::VERSION,
        data_dir: paths::home(),
        profiles_dir: paths::user_profiles_dir(),
        packs_dir: paths::user_packs_dir(),
        results_dir: paths::results_dir(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            probe,
            list_profiles,
            plan_profile,
            start_run,
            cancel_run,
            list_results,
            load_result,
            list_packs,
            app_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running InferMark");
}
