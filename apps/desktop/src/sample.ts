// Sample data for previewing the UI in a plain browser (no Tauri backend). Never used in the app.
import type {
  AppInfo,
  HardwareInfo,
  PackSummary,
  PlanEntry,
  ProfileEntry,
  ResultSummary,
  RunListeners,
  RunResult,
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
      descriptor: null,
      status: { status: "skipped", reason: "not available in this build of InferMark" },
    },
  ];
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
  modules: [
    {
      module_id: "system-probe",
      status: { status: "completed" },
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
      status: { status: "skipped", reason: "not available in this build of InferMark" },
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
  listeners?.onEvent({ type: "module-started", module_id: steps[0].module_id, index: 0, total: steps.length });
  timer = window.setInterval(() => {
    const step = steps[i];
    pct += 0.25;
    listeners?.onEvent({
      type: "module-progress",
      module_id: step.module_id,
      progress: { fraction: Math.min(pct, 1), message: `Pass ${Math.round(pct * 4)} of 4` },
    });
    if (pct >= 1) {
      listeners?.onEvent({ type: "module-finished", record: sampleResult.modules[i] });
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
