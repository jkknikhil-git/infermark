//! Tauri commands: a thin layer over `infermark-core`, so the app and the CLI behave the same.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use infermark_core::engine::{self, Accel, EngineCatalog, EngineStatus, EngineStore};
use infermark_core::models::download::{self, DownloadPolicy, Progress};
use infermark_core::models::{Catalog, ModelStore};
use infermark_core::{
    model_list, packs, paths, plan, profiles, run as run_benchmark, settings, Registry, RunEvent,
};
use infermark_sdk::{GateSettings, HardwareInfo, ModuleStatus, PlanEntry, RunResult};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    /// Cancel flag of the run in progress, if any.
    running: Mutex<Option<Arc<AtomicBool>>>,
    /// Cancel flags of downloads in progress, by model id (or `engine`).
    downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
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

/// Memory and power right now; the Device page polls this every couple of seconds.
#[tauri::command]
async fn live_status() -> infermark_hwprobe::LiveStatus {
    tauri::async_runtime::spawn_blocking(infermark_hwprobe::live_status)
        .await
        .expect("live status never panics")
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

// ---------- Models and engines ----------

#[derive(Serialize)]
struct ModelsView {
    models: Vec<model_list::ModelEntry>,
    models_dir: PathBuf,
    /// Free space on the drive that holds the models folder.
    free_bytes: Option<u64>,
    /// Whether speed predictions are available (needs one memory-speed result).
    speed_known: bool,
}

#[tauri::command]
async fn list_models() -> CmdResult<ModelsView> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = settings::load().map_err(err)?;
        let hw = infermark_hwprobe::probe();
        let bandwidth = model_list::latest_bandwidth(&hw.fingerprint);
        let store = ModelStore::new(s.models_dir());
        let models = model_list::list(&hw, &GateSettings::default(), bandwidth, &store);
        let free_bytes = existing_ancestor(store.root()).and_then(|p| download::free_space(&p));
        Ok(ModelsView {
            models,
            models_dir: store.root().to_path_buf(),
            free_bytes,
            speed_known: bandwidth.is_some(),
        })
    })
    .await
    .map_err(err)?
}

/// The folder itself, or its closest parent that exists (for free-space checks).
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.exists()).map(Path::to_path_buf)
}

#[derive(Serialize, Clone)]
struct DownloadEvent {
    /// Catalog model id, or `engine`.
    id: String,
    #[serde(flatten)]
    progress: Progress,
}

#[derive(Serialize, Clone)]
struct DownloadDone {
    id: String,
    /// `None` on success.
    error: Option<String>,
}

type DownloadWork =
    Box<dyn FnOnce(&mut dyn FnMut(Progress), &AtomicBool) -> Result<(), String> + Send>;

/// Run a download on a background thread. Progress arrives as `download-progress` events and
/// the outcome as `download-done`.
fn spawn_download(
    id: String,
    app: AppHandle,
    state: &AppState,
    work: DownloadWork,
) -> CmdResult<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = state.downloads.lock().unwrap();
        if downloads.contains_key(&id) {
            return Err("Already downloading.".into());
        }
        downloads.insert(id.clone(), cancel.clone());
    }
    std::thread::spawn(move || {
        let emitter = app.clone();
        let event_id = id.clone();
        let mut on_progress = move |progress: Progress| {
            let _ = emitter.emit(
                "download-progress",
                DownloadEvent {
                    id: event_id.clone(),
                    progress,
                },
            );
        };
        let result = work(&mut on_progress, &cancel);
        if let Some(state) = app.try_state::<AppState>() {
            state.downloads.lock().unwrap().remove(&id);
        }
        let _ = app.emit(
            "download-done",
            DownloadDone {
                id,
                error: result.err(),
            },
        );
    });
    Ok(())
}

#[tauri::command]
fn download_model(id: String, app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let model = Catalog::builtin()
        .get(&id)
        .cloned()
        .ok_or("Unknown model.")?;
    let store = ModelStore::new(settings::load().map_err(err)?.models_dir());
    spawn_download(
        id,
        app,
        &state,
        Box::new(move |on_progress, cancel| {
            store
                .install(&model, &DownloadPolicy::default(), on_progress, cancel)
                .map(|_| ())
                .map_err(err)
        }),
    )
}

#[tauri::command]
fn cancel_download(id: String, state: State<'_, AppState>) {
    if let Some(flag) = state.downloads.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn delete_model(id: String, state: State<'_, AppState>) -> CmdResult<()> {
    if state.downloads.lock().unwrap().contains_key(&id) {
        return Err("Stop the download first.".into());
    }
    let model = Catalog::builtin()
        .get(&id)
        .cloned()
        .ok_or("Unknown model.")?;
    ModelStore::new(settings::load().map_err(err)?.models_dir())
        .remove(&model)
        .map_err(err)
}

#[derive(Serialize)]
struct EngineBuildInfo {
    id: String,
    build: String,
    accel: Accel,
    accel_label: &'static str,
    download_bytes: u64,
    installed: bool,
    recommended: bool,
}

#[derive(Serialize)]
struct EngineView {
    status: EngineStatus,
    builds: Vec<EngineBuildInfo>,
}

#[tauri::command]
async fn engine_status() -> CmdResult<EngineView> {
    tauri::async_runtime::spawn_blocking(|| {
        let s = settings::load().map_err(err)?;
        let catalog = EngineCatalog::builtin();
        let store = EngineStore::new(s.engines_dir());
        let recommended = catalog
            .recommended(&infermark_hwprobe::probe())
            .map(|b| b.id.clone());
        let builds = catalog
            .for_this_platform()
            .into_iter()
            .map(|b| EngineBuildInfo {
                id: b.id.clone(),
                build: b.build.clone(),
                accel: b.accel,
                accel_label: b.accel.label(),
                download_bytes: b.download_bytes(),
                installed: store.server_path(b).is_some(),
                recommended: recommended.as_deref() == Some(b.id.as_str()),
            })
            .collect();
        Ok(EngineView {
            status: engine::status(&s.engine_config()),
            builds,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn install_engine(id: String, app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let build = EngineCatalog::builtin()
        .get(&id)
        .cloned()
        .ok_or("Unknown engine build.")?;
    let store = EngineStore::new(settings::load().map_err(err)?.engines_dir());
    spawn_download(
        "engine".into(),
        app,
        &state,
        Box::new(move |on_progress, cancel| {
            store
                .install(
                    &build,
                    &DownloadPolicy::default(),
                    &mut |_, _, p| on_progress(p),
                    cancel,
                )
                .map(|_| ())
                .map_err(err)
        }),
    )
}

// ---------- Settings ----------

#[derive(Serialize)]
struct SettingsView {
    settings: settings::Settings,
    /// The folder actually used (setting, environment override or default).
    storage_dir: PathBuf,
    default_storage_dir: PathBuf,
}

fn settings_view(s: settings::Settings) -> SettingsView {
    SettingsView {
        storage_dir: s.storage_dir(),
        default_storage_dir: paths::home(),
        settings: s,
    }
}

#[tauri::command]
fn get_settings() -> CmdResult<SettingsView> {
    settings::load().map(settings_view).map_err(err)
}

/// Save one setting (`storage_dir`, `llama_server` or `ollama_url`); an empty value resets it.
#[tauri::command]
fn set_setting(key: String, value: String) -> CmdResult<SettingsView> {
    let mut s = settings::load().map_err(err)?;
    s.set(&key, &value).map_err(err)?;
    settings::save(&s).map_err(err)?;
    Ok(settings_view(s))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            probe,
            live_status,
            list_profiles,
            plan_profile,
            start_run,
            cancel_run,
            list_results,
            load_result,
            list_packs,
            app_info,
            list_models,
            download_model,
            cancel_download,
            delete_model,
            engine_status,
            install_engine,
            get_settings,
            set_setting
        ])
        .run(tauri::generate_context!())
        .expect("error while running InferMark");
}
