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

export interface ModuleRecord {
  module_id: string;
  status: ModuleStatus;
  duration_ms: number;
  output: { metrics: Metric[]; series: unknown[]; notes: string[] };
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
}

export type RunEvent =
  | { type: "module-started"; module_id: string; index: number; total: number }
  | { type: "module-progress"; module_id: string; progress: { fraction: number; message: string } }
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

export const isPreview = !("__TAURI_INTERNALS__" in window);

function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: () => T): Promise<T> {
  return isPreview ? Promise.resolve(fallback()) : invoke<T>(cmd, args);
}

export const api = {
  probe: () => call<HardwareInfo>("probe", undefined, () => sample.hardware),
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
};

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
