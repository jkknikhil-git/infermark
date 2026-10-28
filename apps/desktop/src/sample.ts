// Sample data for previewing the UI in a plain browser (no Tauri backend). Never used in the app.
import type {
  AppInfo,
  DownloadListeners,
  EngineView,
  HardwareInfo,
  ModelEntry,
  ModelsView,
  SettingKey,
  SettingsView,
  Tier,
  PackSummary,
  PlanEntry,
  ProfileEntry,
  ResultSummary,
  RunListeners,
  RunResult,
  SensorInventory,
  SensorReading,
} from "./api";

const GIB = 1024 ** 3;

export const hardware: HardwareInfo = {
  fingerprint: "a1b2c3d4e5f60718",
  os: { family: "windows", name: "Windows", version: "11 (26200)", kernel: "26200", arch: "x86_64" },
  cpu: {
    brand: "11th Gen Intel(R) Core(TM) i5-11320H @ 3.20GHz",
    vendor: "GenuineIntel",
    physical_cores: 4,
    logical_cores: 8,
    frequency_mhz: 3187,
  },
  memory: { total_bytes: 16 * GIB, available_bytes: 7.4 * GIB },
  gpus: [
    {
      name: "Intel(R) Iris(R) Xe Graphics",
      vendor: "intel",
      dedicated_vram_bytes: 128 * 1024 ** 2,
      shared_memory_bytes: 7.9 * GIB,
      driver_version: "31.0.101.5186",
      integrated: true,
    },
  ],
  npus: [],
  battery: { charge_percent: 82, design_capacity_wh: 56, full_capacity_wh: 49.3, charging: true },
  power_source: "ac",
};

// Free memory drifts a little so the live view can be previewed.
export function liveStatus() {
  const jitter = (Math.random() - 0.5) * 0.4 * GIB;
  return {
    memory: { ...hardware.memory, available_bytes: hardware.memory.available_bytes + jitter },
    battery: hardware.battery,
    power_source: hardware.power_source,
  };
}

const descriptor = (id: string, name: string, description: string, estimated_seconds: number) => ({
  id,
  name,
  description,
  category: "system",
  requirements: [],
  estimated_seconds,
});

const probeD = descriptor("system-probe", "System check", "Detects your hardware and checks for anything that could skew results.", 1);
const membwD = descriptor("memory-bandwidth", "Memory speed", "Measures how fast data moves between memory and the CPU (copy and triad).", 10);

export const profiles: ProfileEntry[] = [
  ["quick", "Quick", "About 5 minutes. Hardware check, memory speed and one small model."],
  ["standard", "Standard", "About 30 minutes. Speed, context scaling, a short endurance test and core quality packs."],
  ["full", "Full", "Everything your device can run: all eligible models, all packs, 20-minute endurance test."],
  ["laptop-battery", "Laptop battery", "Unplug first. Measures battery drain per 10k tokens and estimates hours of local chat."],
].map(([key, name, description]) => ({
  key,
  source: { kind: "builtin" as const },
  profile: {
    schema: 1,
    name,
    description,
    modules: { "system-probe": { enabled: true }, "memory-bandwidth": { enabled: true }, "inference-perf": { enabled: true } },
  },
}));

export function plan(_key: string): PlanEntry[] {
  return [
    { module_id: "system-probe", descriptor: probeD, status: { status: "planned" } },
    { module_id: "memory-bandwidth", descriptor: membwD, status: { status: "planned" } },
    {
      module_id: "inference-perf",
      descriptor: descriptor("inference-perf", "Inference speed", "How fast each model reads your prompt and writes its answer.", 90),
      status: { status: "planned" },
    },
    {
      module_id: "sustained-load",
      descriptor: descriptor("sustained-load", "Endurance", "Keeps a model writing for several minutes to see whether speed holds up as the device heats up.", 300),
      status: { status: "planned" },
    },
    {
      module_id: "battery-drain",
      descriptor: descriptor("battery-drain", "Battery", "Runs a model on battery power to measure the charge it uses.", 600),
      status: { status: "skipped", reason: "unplug the charger to run battery tests" },
    },
    {
      module_id: "quality-packs",
      descriptor: null,
      status: { status: "skipped", reason: "not available in this build of InferMark" },
    },
  ];
}

/** Five minutes of generation on a laptop that slows from 44 to about 38 tok/s as it warms up. */
const sustainedSpeeds: [number, number][] = Array.from({ length: 100 }, (_, i) => {
  const t = 4 + i * 3;
  return [t, Math.round((38 + 6 * Math.exp(-t / 70) + Math.sin(i) * 0.4) * 100) / 100];
});

// What this preview "device" can measure: like a Windows laptop with an Intel iGPU.
const ch = (source: string, kind: SensorReading["kind"], label: string) => ({ source, kind, label });
const sensors: SensorInventory = {
  available: [
    ch("cpu", "utilization", "CPU load"),
    ch("cpu", "clock", "CPU clock"),
    ch("battery", "battery-charge", "Battery charge"),
    ch("battery", "power", "Battery drain"),
  ],
  missing: [
    ch("cpu", "temperature", "CPU temperature"),
    ch("cpu", "power", "CPU package power"),
    ch("gpu", "utilization", "GPU load"),
    ch("gpu", "temperature", "GPU temperature"),
    ch("gpu", "power", "GPU power"),
    ch("battery", "temperature", "Battery temperature"),
    ch("device", "thermal-state", "Thermal state"),
  ],
};

/** Plausible readings `t` seconds into a run (plugged in, so no battery drain). */
function readingsAt(t: number): SensorReading[] {
  const busy = t > 6;
  return [
    { source: "cpu", kind: "utilization", value: t === 0 ? null : Math.round((busy ? 62 : 18) + Math.sin(t / 3) * 9) },
    { source: "cpu", kind: "clock", value: Math.round((busy ? 2950 : 2400) - Math.min(t, 240) * 2 + Math.cos(t / 2) * 60) },
    { source: "battery", kind: "battery-charge", value: Math.min(82 + t * 0.01, 100) },
    { source: "battery", kind: "power", value: null },
  ];
}

function telemetryFor(seconds: number): RunResult["telemetry"] {
  const series = new Map<string, [number, number][]>();
  for (let t = 0; t <= seconds; t++) {
    for (const r of readingsAt(t)) {
      if (r.value === null) continue;
      const key = `${r.source}.${r.kind}`;
      series.set(key, [...(series.get(key) ?? []), [t, r.value]]);
    }
  }
  const unit: Record<string, string> = { utilization: "%", clock: "MHz", "battery-charge": "%" };
  return {
    sensors,
    interval_ms: 1000,
    series: [...series].map(([key, points]) => {
      const c = sensors.available.find((a) => `${a.source}.${a.kind}` === key)!;
      return { key, label: c.label, x_unit: "s", y_unit: unit[c.kind] ?? "", points };
    }),
  };
}

const sampleResult: RunResult = {
  schema: 1,
  infermark_version: "0.1.0",
  run_id: "00000000-0000-4000-8000-000000000001",
  started_at: new Date(Date.now() - 3600_000).toISOString(),
  finished_at: new Date(Date.now() - 3588_000).toISOString(),
  hardware,
  profile: profiles[0].profile,
  plan: plan("quick"),
  telemetry: telemetryFor(330),
  modules: [
    {
      module_id: "system-probe",
      status: { status: "completed" },
      offset_ms: 0,
      duration_ms: 40,
      output: {
        metrics: [
          { key: "logical_cores", label: "CPU threads", value: 8, unit: "", direction: "neutral" },
          { key: "ram_total_gb", label: "Memory", value: 16, unit: "GB", direction: "neutral" },
          { key: "gpu_count", label: "GPUs", value: 1, unit: "", direction: "neutral" },
        ],
        series: [],
        notes: [],
      },
    },
    {
      module_id: "memory-bandwidth",
      status: { status: "completed" },
      offset_ms: 40,
      duration_ms: 6200,
      output: {
        metrics: [
          { key: "copy_gbps", label: "Copy bandwidth", value: 24.8, unit: "GB/s", direction: "higher-is-better" },
          { key: "triad_gbps", label: "Triad bandwidth", value: 27.3, unit: "GB/s", direction: "higher-is-better" },
        ],
        series: [],
        notes: [],
      },
    },
    {
      module_id: "inference-perf",
      status: { status: "completed" },
      offset_ms: 6240,
      duration_ms: 21000,
      output: {
        metrics: [
          { key: "qwen3-0.6b-q4/gen_tps", label: "Qwen3 0.6B · Generation", value: 44.9, unit: "tok/s", direction: "higher-is-better" },
          { key: "qwen3-0.6b-q4/prompt_tps", label: "Qwen3 0.6B · Prompt processing", value: 490.7, unit: "tok/s", direction: "higher-is-better" },
          { key: "qwen3-0.6b-q4/ttft_ms", label: "Qwen3 0.6B · Time to first token", value: 1199, unit: "ms", direction: "lower-is-better" },
          { key: "qwen3-0.6b-q4/load_s", label: "Qwen3 0.6B · Load time", value: 2.1, unit: "s", direction: "lower-is-better" },
        ],
        series: [],
        notes: ["Engine: llama.cpp b11669 on Intel Iris Xe Graphics (Vulkan)"],
      },
    },
    {
      module_id: "sustained-load",
      status: { status: "completed" },
      offset_ms: 27240,
      duration_ms: 302000,
      output: {
        metrics: [
          { key: "stability_pct", label: "Stability", value: 86.6, unit: "%", direction: "higher-is-better" },
          { key: "throttle_pct", label: "Throttling", value: 13.4, unit: "%", direction: "lower-is-better" },
          { key: "peak_tps", label: "Peak speed", value: 44.1, unit: "tok/s", direction: "higher-is-better" },
          { key: "sustained_tps", label: "Sustained speed", value: 38.2, unit: "tok/s", direction: "higher-is-better" },
          { key: "burst_tps", label: "Burst speed (short test)", value: 44.9, unit: "tok/s", direction: "higher-is-better" },
          { key: "loops", label: "Generations", value: 100, unit: "", direction: "neutral" },
          { key: "tokens", label: "Tokens written", value: 12800, unit: "", direction: "neutral" },
        ],
        series: [{ key: "tps", label: "Speed over time", x_unit: "s", y_unit: "tok/s", points: sustainedSpeeds }],
        notes: ["Qwen3 0.6B: 100 generations over 5.1 min, peak 44.1 tok/s, last quarter 38.2 tok/s."],
      },
    },
    {
      module_id: "battery-drain",
      status: { status: "skipped", reason: "unplug the charger to run battery tests" },
      offset_ms: 329240,
      duration_ms: 0,
      output: { metrics: [], series: [], notes: [] },
    },
  ],
};

let saved: ResultSummary[] = [
  {
    path: "sample.json",
    run_id: sampleResult.run_id,
    profile_name: "Quick",
    started_at: sampleResult.started_at,
    duration_ms: 12000,
    completed: 2,
    skipped: 1,
    failed: 0,
  },
];

export const results = () => saved;
export const loadResult = (_path: string) => sampleResult;

export const packs: PackSummary[] = [];

export const appInfo: AppInfo = {
  version: "0.1.0 (preview)",
  data_dir: "~/.infermark",
  profiles_dir: "~/.infermark/profiles",
  packs_dir: "~/.infermark/packs",
  results_dir: "~/.infermark/results",
};

// Simulated run so the live run screen can be previewed.
let listeners: RunListeners | null = null;
let timer: number | undefined;

export function listen(l: RunListeners) {
  listeners = l;
  return () => {
    // Only clear our own registration (React StrictMode mounts effects twice).
    if (listeners === l) listeners = null;
  };
}

export function startRun(_key: string) {
  const steps = plan(_key);
  let i = 0;
  let pct = 0;
  let t = 0;
  listeners?.onEvent({ type: "run-started", sensors });
  listeners?.onEvent({ type: "sensors", t, readings: readingsAt(t) });
  listeners?.onEvent({ type: "module-started", module_id: steps[0].module_id, index: 0, total: steps.length });
  timer = window.setInterval(() => {
    const step = steps[i];
    t += 1;
    listeners?.onEvent({ type: "sensors", t, readings: readingsAt(t) });
    pct += 0.125;
    const pass = Math.round(pct * 8);
    const live =
      step.module_id === "inference-perf" && pass > 2
        ? { label: "Qwen3 0.6B · Generation", value: Math.round((45 - pass * 0.4 + Math.random()) * 10) / 10, unit: "tok/s" }
        : undefined;
    listeners?.onEvent({
      type: "module-progress",
      module_id: step.module_id,
      progress: { fraction: Math.min(pct, 1), message: `Pass ${pass} of 8`, live },
    });
    if (pct >= 1) {
      const record = sampleResult.modules.find((m) => m.module_id === step.module_id) ?? {
        module_id: step.module_id,
        status: { status: "skipped" as const, reason: "coming in a future update" },
        duration_ms: 0,
        output: { metrics: [], series: [], notes: [] },
      };
      listeners?.onEvent({ type: "module-finished", record });
      i += 1;
      pct = 0;
      if (i >= steps.length) {
        window.clearInterval(timer);
        const summary = { ...saved[0], started_at: new Date().toISOString() };
        saved = [summary, ...saved];
        listeners?.onFinished(summary);
        return;
      }
      listeners?.onEvent({ type: "module-started", module_id: steps[i].module_id, index: i, total: steps.length });
    }
  }, 500);
}

export function cancelRun() {
  window.clearInterval(timer);
  listeners?.onError("Benchmark stopped.");
}

// ---------- Models (preview only) ----------

const model = (
  id: string,
  name: string,
  family: string,
  params_b: number,
  size_bytes: number,
  license: string,
  tier: Tier,
  tps: number | null,
  summary: string,
  installed = false,
): ModelEntry => ({
  id,
  name,
  family,
  params_b,
  quant: "Q4_K_M",
  license,
  tags: ["chat"],
  url: "",
  size_bytes,
  sha256: "",
  installed,
  partial_bytes: 0,
  verdict: { tier, needed_bytes: size_bytes * 1.3, budget_bytes: 13.4 * GIB, predicted_tokens_per_sec: tps, summary },
});

let models: ModelEntry[] = [
  model("qwen3-0.6b-q4", "Qwen3 0.6B", "Qwen3", 0.6, 396705472, "Apache-2.0", "system-memory", 26, "Runs in system memory, shared with the integrated GPU (0.9 GB of 13.4 GB)", true),
  model("llama-3.2-1b-q4", "Llama 3.2 1B Instruct", "Llama 3", 1.2, 807694464, "Llama 3.2 Community", "system-memory", 13, "Runs in system memory, shared with the integrated GPU (1.0 GB of 13.4 GB)"),
  model("llama-3.2-3b-q4", "Llama 3.2 3B Instruct", "Llama 3", 3.2, 2019377696, "Llama 3.2 Community", "system-memory", 5, "Runs in system memory, shared with the integrated GPU (2.7 GB of 13.4 GB)"),
  model("qwen3-8b-q4", "Qwen3 8B", "Qwen3", 8.2, 5027784512, "Apache-2.0", "system-memory", 2, "Runs in system memory, shared with the integrated GPU (6.0 GB of 13.4 GB)"),
  model("qwen3-14b-q4", "Qwen3 14B", "Qwen3", 14.8, 9001753984, "Apache-2.0", "too-slow", 1.1, "Predicted about 1.1 tokens/s, below the 2 tokens/s minimum"),
  model("llama-3.3-70b-q4", "Llama 3.3 70B Instruct", "Llama 3", 70.6, 42520398816, "Llama 3.3 Community", "too-big", null, "Needs 47.0 GB, this device can use 13.4 GB"),
];

export const modelsView = (): ModelsView => ({
  models,
  models_dir: "~/.infermark/models",
  free_bytes: 154 * 1e9,
  speed_known: true,
});

let dlListeners: DownloadListeners | null = null;
const dlTimers = new Map<string, number>();

export function listenDownloads(l: DownloadListeners) {
  dlListeners = l;
  return () => {
    if (dlListeners === l) dlListeners = null;
  };
}

function simulate(id: string, total: number, onDone: () => void) {
  let done = 0;
  const timer = window.setInterval(() => {
    done = Math.min(total, done + total / 12);
    dlListeners?.onProgress({ id, phase: done < total ? "downloading" : "verifying", done, total });
    if (done >= total) {
      window.clearInterval(timer);
      dlTimers.delete(id);
      onDone();
      dlListeners?.onDone({ id, error: null });
    }
  }, 300);
  dlTimers.set(id, timer);
}

export function startDownload(id: string) {
  const m = models.find((x) => x.id === id);
  if (!m) return;
  simulate(id, m.size_bytes, () => {
    models = models.map((x) => (x.id === id ? { ...x, installed: true } : x));
  });
}

export function cancelDownload(id: string) {
  window.clearInterval(dlTimers.get(id));
  dlTimers.delete(id);
  dlListeners?.onDone({ id, error: "cancelled" });
}

export function deleteModel(id: string) {
  models = models.map((x) => (x.id === id ? { ...x, installed: false } : x));
}

let engineInstalled = false;

export const engineView = (): EngineView => ({
  status: {
    llama_cpp: engineInstalled ? { path: "~/.infermark/engines/llama-server", version: "b11669", accel: "vulkan" } : null,
    ollama: null,
  },
  builds: [
    { id: "vulkan", build: "b11669", accel: "vulkan", accel_label: "Vulkan (GPU)", download_bytes: 32182350, installed: engineInstalled, recommended: true },
    { id: "cpu", build: "b11669", accel: "cpu", accel_label: "CPU", download_bytes: 19537886, installed: false, recommended: false },
  ],
});

export function installEngine(_id: string) {
  simulate("engine", 32182350, () => {
    engineInstalled = true;
  });
}

let settings: SettingsView = {
  settings: { ollama_url: "http://127.0.0.1:11434" },
  storage_dir: "~/.infermark",
  default_storage_dir: "~/.infermark",
};

export const settingsView = () => settings;

export function setSetting(key: SettingKey, value: string): SettingsView {
  const s = { ...settings.settings, [key]: value || undefined };
  if (key === "ollama_url" && !value) s.ollama_url = "http://127.0.0.1:11434";
  settings = {
    ...settings,
    settings: s as SettingsView["settings"],
    storage_dir: key === "storage_dir" ? value || settings.default_storage_dir : settings.storage_dir,
  };
  return settings;
}
