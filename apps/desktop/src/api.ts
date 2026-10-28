// Typed wrappers around the Tauri commands in src-tauri/src/lib.rs. Types mirror the Rust
// structs in crates/infermark-sdk (serde field names).
//
// When the UI is opened in a plain browser (`npm run dev` without Tauri) the calls fall back to
// sample data so the interface can be developed and reviewed without building the Rust side.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as sample from "./sample";

export type GpuVendor = "nvidia" | "amd" | "intel" | "apple" | "qualcomm" | "arm" | "microsoft" | "other";

export interface HardwareInfo {
  fingerprint: string;
  os: { family: string; name: string; version: string; kernel: string; arch: string };
  cpu: {
    brand: string;
    vendor: string;
    physical_cores: number | null;
    logical_cores: number;
    frequency_mhz: number | null;
  };
  memory: { total_bytes: number; available_bytes: number };
  gpus: {
    name: string;
    vendor: GpuVendor;
    dedicated_vram_bytes: number | null;
    shared_memory_bytes: number | null;
    driver_version: string | null;
    integrated: boolean;
  }[];
  npus: { name: string; vendor: string }[];
  battery: {
    charge_percent: number;
    design_capacity_wh: number | null;
    full_capacity_wh: number | null;
    charging: boolean;
  } | null;
  power_source: "ac" | "battery" | "unknown";
}

export type LiveStatus = Pick<HardwareInfo, "memory" | "battery" | "power_source">;

export interface ModuleDescriptor {
  id: string;
  name: string;
  description: string;
  category: string;
  requirements: unknown[];
  estimated_seconds: number;
}

export type PlanStatus = { status: "planned" } | { status: "skipped"; reason: string };

export interface PlanEntry {
  module_id: string;
  descriptor: ModuleDescriptor | null;
  status: PlanStatus;
}

export interface Profile {
  schema: number;
  name: string;
  description: string;
  modules: Record<string, { enabled: boolean; [setting: string]: unknown }>;
}

export interface ProfileEntry {
  key: string;
  source: { kind: "builtin" } | { kind: "user"; path: string };
  profile: Profile;
}

export interface Metric {
  key: string;
  label: string;
  value: number;
  unit: string;
  direction: "higher-is-better" | "lower-is-better" | "neutral";
}

export type ModuleStatus =
  | { status: "completed" }
  | { status: "skipped"; reason: string }
  | { status: "failed"; error: string }
  | { status: "cancelled" };

export interface Series {
  key: string;
  label: string;
  x_unit: string;
  y_unit: string;
  points: [number, number][];
}

export interface ModuleRecord {
  module_id: string;
  status: ModuleStatus;
  /** When the module started, ms after the run started (absent in older results). */
  offset_ms?: number;
  duration_ms: number;
  output: { metrics: Metric[]; series: Series[]; notes: string[] };
}

export interface RunResult {
  schema: number;
  infermark_version: string;
  run_id: string;
  started_at: string;
  finished_at: string;
  hardware: HardwareInfo;
  profile: Profile;
  plan: PlanEntry[];
  modules: ModuleRecord[];
  /** Sensor data for the whole run; absent in older results. */
  telemetry?: Telemetry;
}

export interface SensorChannel {
  source: string;
  kind: "temperature" | "clock" | "utilization" | "power" | "memory-used" | "battery-charge" | "thermal-state";
  label: string;
}

export interface SensorInventory {
  available: SensorChannel[];
  missing: SensorChannel[];
}

export interface SensorReading {
  source: string;
  kind: SensorChannel["kind"];
  /** null when it couldn't be read this time. */
  value: number | null;
}

export interface LiveValue {
  label: string;
  value: number;
  unit: string;
}

export interface Telemetry {
  sensors: SensorInventory;
  interval_ms: number;
  series: Series[];
}

export type RunEvent =
  | { type: "run-started"; sensors: SensorInventory }
  | { type: "sensors"; t: number; readings: SensorReading[] }
  | { type: "module-started"; module_id: string; index: number; total: number }
  | { type: "module-progress"; module_id: string; progress: { fraction: number; message: string; live?: LiveValue } }
  | { type: "module-finished"; record: ModuleRecord };

export interface ResultSummary {
  path: string;
  run_id: string;
  profile_name: string;
  started_at: string;
  duration_ms: number;
  completed: number;
  skipped: number;
  failed: number;
}

export interface PackSummary {
  id: string;
  name: string;
  category: string;
  version: string;
  description: string;
  items: number;
  scorer: string;
  subjective: boolean;
  dir: string;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  profiles_dir: string;
  packs_dir: string;
  results_dir: string;
}

export type Tier = "full-gpu" | "partial-offload" | "system-memory" | "too-big" | "too-slow";

export interface ModelEntry {
  id: string;
  name: string;
  family: string;
  params_b: number;
  quant: string;
  license: string;
  tags: string[];
  url: string;
  size_bytes: number;
  sha256: string;
  ollama?: string;
  installed: boolean;
  partial_bytes: number;
  verdict: {
    tier: Tier;
    needed_bytes: number;
    budget_bytes: number;
    predicted_tokens_per_sec: number | null;
    summary: string;
  };
}

export interface ModelsView {
  models: ModelEntry[];
  models_dir: string;
  free_bytes: number | null;
  speed_known: boolean;
}

export type Accel = "cpu" | "vulkan" | "cuda" | "metal";

export interface EngineView {
  status: {
    llama_cpp: { path: string; version: string; accel: Accel | null } | null;
    ollama: { url: string; version: string; models: { name: string; size_bytes: number }[] } | null;
  };
  builds: {
    id: string;
    build: string;
    accel: Accel;
    accel_label: string;
    download_bytes: number;
    installed: boolean;
    recommended: boolean;
  }[];
}

export type DownloadProgress =
  | { id: string; phase: "downloading"; done: number; total: number }
  | { id: string; phase: "verifying"; done: number; total: number };

export interface DownloadDone {
  id: string;
  error: string | null;
}

export interface SettingsView {
  settings: { storage_dir?: string; llama_server?: string; ollama_url: string };
  storage_dir: string;
  default_storage_dir: string;
}

export type SettingKey = "storage_dir" | "llama_server" | "ollama_url";

export const isPreview = !("__TAURI_INTERNALS__" in window);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  return isPreview ? Promise.resolve(fallback()) : invoke<T>(cmd, args);
}

export const api = {
  probe: () => call<HardwareInfo>("probe", undefined, () => sample.hardware),
  liveStatus: () => call<LiveStatus>("live_status", undefined, () => sample.liveStatus()),
  listProfiles: () =>
    call<{ profiles: ProfileEntry[]; errors: [string, string][] }>("list_profiles", undefined, () => ({
      profiles: sample.profiles,
      errors: [],
    })),
  planProfile: (key: string) => call<PlanEntry[]>("plan_profile", { key }, () => sample.plan(key)),
  startRun: (key: string) => call<void>("start_run", { key }, () => sample.startRun(key)),
  cancelRun: () => call<void>("cancel_run", undefined, () => sample.cancelRun()),
  listResults: () => call<ResultSummary[]>("list_results", undefined, () => sample.results()),
  loadResult: (path: string) => call<RunResult>("load_result", { path }, () => sample.loadResult(path)),
  listPacks: () =>
    call<{ packs: PackSummary[]; errors: string[] }>("list_packs", undefined, () => ({
      packs: sample.packs,
      errors: [],
    })),
  appInfo: () => call<AppInfo>("app_info", undefined, () => sample.appInfo),
  listModels: () => call<ModelsView>("list_models", undefined, () => sample.modelsView()),
  downloadModel: (id: string) => call<void>("download_model", { id }, () => sample.startDownload(id)),
  cancelDownload: (id: string) => call<void>("cancel_download", { id }, () => sample.cancelDownload(id)),
  deleteModel: (id: string) => call<void>("delete_model", { id }, () => sample.deleteModel(id)),
  engineStatus: () => call<EngineView>("engine_status", undefined, () => sample.engineView()),
  installEngine: (id: string) => call<void>("install_engine", { id }, () => sample.installEngine(id)),
  getSettings: () => call<SettingsView>("get_settings", undefined, () => sample.settingsView()),
  setSetting: (key: SettingKey, value: string) =>
    call<SettingsView>("set_setting", { key, value }, () => sample.setSetting(key, value)),
};

export interface DownloadListeners {
  onProgress: (p: DownloadProgress) => void;
  onDone: (d: DownloadDone) => void;
}

export async function listenToDownloads(l: DownloadListeners): Promise<UnlistenFn> {
  if (isPreview) return sample.listenDownloads(l);
  const offs = await Promise.all([
    listen<DownloadProgress>("download-progress", (e) => l.onProgress(e.payload)),
    listen<DownloadDone>("download-done", (e) => l.onDone(e.payload)),
  ]);
  return () => offs.forEach((off) => off());
}

export interface RunListeners {
  onEvent: (e: RunEvent) => void;
  onFinished: (summary: ResultSummary) => void;
  onError: (message: string) => void;
}

export async function listenToRuns(l: RunListeners): Promise<UnlistenFn> {
  if (isPreview) return sample.listen(l);
  const offs = await Promise.all([
    listen<RunEvent>("run-event", (e) => l.onEvent(e.payload)),
    listen<ResultSummary>("run-finished", (e) => l.onFinished(e.payload)),
    listen<string>("run-error", (e) => l.onError(e.payload)),
  ]);
  return () => offs.forEach((off) => off());
}

// Formatting helpers shared by pages.
export const GIB = 1024 ** 3;
export const gb = (bytes: number | null | undefined, digits = 1) =>
  bytes == null ? "Unknown" : `${(bytes / GIB).toFixed(digits)} GB`;

/** Download sizes, in decimal units like browsers and stores show them. */
export const size = (bytes: number) =>
  bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;

export function formatDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ${s % 60}s` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}
