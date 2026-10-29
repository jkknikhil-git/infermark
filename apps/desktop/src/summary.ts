// Turns a raw result (modules -> flat metric lists) into what people read: categories like
// Speed and Responsiveness, each with a headline number and the per-model details.

import type { Metric, ModuleRecord, RunResult } from "./api";
import { sensorCharts } from "./sensor-charts";

export type CategoryId = "speed" | "responsiveness" | "endurance" | "efficiency" | "quality";

export interface CategoryDef {
  id: CategoryId;
  name: string;
  icon: string;
  /** What the category tells you, in plain words. */
  blurb: string;
  /** Modules that feed this category. */
  modules: string[];
}

export const CATEGORIES: CategoryDef[] = [
  {
    id: "speed",
    name: "Speed",
    icon: "speed",
    blurb: "How fast models write answers and read your prompt",
    modules: ["inference-perf", "thread-sweep", "memory-bandwidth"],
  },
  {
    id: "responsiveness",
    name: "Responsiveness",
    icon: "timer",
    blurb: "How long until the first word appears, and load time",
    modules: ["inference-perf"],
  },
  {
    id: "endurance",
    name: "Endurance",
    icon: "thermostat",
    blurb: "Whether speed holds up as the device heats up",
    modules: ["sustained-load"],
  },
  {
    id: "efficiency",
    name: "Battery",
    icon: "battery_charging_full",
    blurb: "Battery used per answer and hours of local chat",
    modules: ["battery-drain"],
  },
  {
    id: "quality",
    name: "Answer quality",
    icon: "fact_check",
    blurb: "How good the answers are: coding, reasoning, summaries and more",
    modules: ["quality-packs"],
  },
];

/** Metric key suffixes (after `model/`) that belong to Responsiveness rather than Speed. */
const RESPONSIVENESS_KEYS = new Set(["ttft_ms", "load_s"]);

export function categoryOf(moduleId: string, metricKey: string): CategoryId | null {
  const suffix = metricKey.includes("/") ? metricKey.slice(metricKey.indexOf("/") + 1) : metricKey;
  if (moduleId === "inference-perf") return RESPONSIVENESS_KEYS.has(suffix) ? "responsiveness" : "speed";
  return CATEGORIES.find((c) => c.modules.includes(moduleId))?.id ?? null;
}

export interface MetricGroup {
  /** Model name for per-model metrics, otherwise the test name. */
  title: string;
  metrics: { key: string; label: string; value: number; unit: string; direction: Metric["direction"] }[];
}

export interface CategoryView {
  def: CategoryDef;
  groups: MetricGroup[];
  /** e.g. "44.9 tok/s · Qwen3 0.6B"; null when nothing was measured. */
  headline: string | null;
  /** Why nothing was measured (skipped reason, not in this profile, coming later). */
  missing: string | null;
  /** Curves worth seeing next to the numbers (speed over time, temperature, charge). */
  charts: CategoryChart[];
}

export interface CategoryChart {
  title: string;
  unit: string;
  points: [number, number][];
  zero: boolean;
  minSpan: number;
  format?: (value: number) => string;
}

/** Metrics like `qwen3-0.6b-q4/gen_tps` labelled "Qwen3 0.6B · Generation" are grouped by model. */
function groupMetrics(record: ModuleRecord, moduleName: string, category: CategoryId): MetricGroup[] {
  const groups = new Map<string, MetricGroup>();
  for (const m of record.output.metrics) {
    if (categoryOf(record.module_id, m.key) !== category) continue;
    const perModel = m.key.includes("/") && m.label.includes(" · ");
    const [title, label] = perModel ? splitOnce(m.label, " · ") : [moduleName, m.label];
    const group = groups.get(title) ?? { title, metrics: [] };
    group.metrics.push({ key: m.key, label, value: m.value, unit: m.unit, direction: m.direction });
    groups.set(title, group);
  }
  return [...groups.values()];
}

function splitOnce(s: string, sep: string): [string, string] {
  const i = s.indexOf(sep);
  return [s.slice(0, i), s.slice(i + sep.length)];
}

function headlineFor(category: CategoryId, groups: MetricGroup[]): string | null {
  const all = groups.flatMap((g) => g.metrics.map((m) => ({ ...m, group: g.title })));
  if (all.length === 0) return null;
  const pick = (suffix: string, best: "max" | "min") => {
    const found = all.filter((m) => m.key.endsWith(`/${suffix}`));
    if (found.length === 0) return null;
    const m = found.reduce((a, b) => ((best === "max" ? b.value > a.value : b.value < a.value) ? b : a));
    return `${formatNumber(m.value)} ${m.unit} · ${m.group}`;
  };
  if (category === "speed") return pick("gen_tps", "max") ?? `${formatNumber(all[0].value)} ${all[0].unit} · ${all[0].label}`;
  if (category === "responsiveness") return pick("ttft_ms", "min");
  const value = (key: string) => all.find((m) => m.key === key)?.value;
  if (category === "endurance") {
    const stability = value("stability_pct");
    const peak = value("peak_tps");
    const sustained = value("sustained_tps");
    if (stability !== undefined && peak !== undefined && sustained !== undefined)
      return `${formatNumber(stability)}% stability · ${formatNumber(peak)} → ${formatNumber(sustained)} tok/s`;
  }
  if (category === "efficiency") {
    const hours = value("hours_nonstop");
    const per10k = value("pct_per_10k_tokens");
    const parts = [
      hours !== undefined ? `${formatNumber(hours)} h nonstop` : null,
      per10k !== undefined ? `${formatNumber(per10k)}% per 10k tokens` : null,
    ].filter(Boolean);
    if (parts.length > 0) return parts.join(" · ");
    const used = value("charge_used_pct");
    if (used !== undefined) return `${formatNumber(used)}% charge used`;
  }
  return `${formatNumber(all[0].value)} ${all[0].unit} · ${all[0].label}`;
}

export function categorize(result: RunResult): CategoryView[] {
  const names = new Map(result.plan.map((p) => [p.module_id, p.descriptor?.name ?? p.module_id]));
  return CATEGORIES.map((def) => {
    // In the category's own order: model speed before memory speed.
    const records = result.modules
      .filter((m) => def.modules.includes(m.module_id))
      .sort((a, b) => def.modules.indexOf(a.module_id) - def.modules.indexOf(b.module_id));
    const groups = records.flatMap((r) => groupMetrics(r, names.get(r.module_id) ?? r.module_id, def.id));
    let missing: string | null = null;
    if (groups.length === 0) {
      const reasons = records
        .map((r) =>
          r.status.status === "skipped"
            ? r.status.reason
            : r.status.status === "failed"
              ? `failed: ${r.status.error}`
              : r.status.status === "cancelled"
                ? "stopped before it ran"
                : null,
        )
        .filter((r): r is string => !!r);
      missing = reasons[0] ?? "not part of this profile";
      if (/not available in this build/.test(missing)) missing = "coming in a future update";
    }
    return { def, groups, headline: headlineFor(def.id, groups), missing, charts: chartsFor(def.id, records, result) };
  });
}

/** Sensor kinds drawn under each category, first available wins (one y-axis per chart). */
const CATEGORY_SENSORS: Partial<Record<CategoryId, string[][]>> = {
  // Temperature where the device reports it; otherwise clock speed, which also shows throttling.
  endurance: [["cpu.temperature", "gpu0.temperature", "device.thermal-state", "cpu.clock"]],
  efficiency: [["battery.power"]],
};

function chartsFor(category: CategoryId, records: ModuleRecord[], result: RunResult): CategoryChart[] {
  // Only curves over time; speed per run or per thread count reads better as numbers.
  if (!CATEGORY_SENSORS[category]) return [];
  const record = records.find((r) => r.status.status === "completed" && r.output.series.length > 0);
  if (!record) return [];
  const charts: CategoryChart[] = record.output.series.map((s) => ({
    title: s.label,
    unit: s.y_unit,
    points: s.points,
    zero: s.y_unit === "tok/s",
    minSpan: s.y_unit === "%" ? 5 : 0,
  }));
  // The run's sensors during this module only, on the module's own clock.
  const telemetry = result.telemetry;
  if (!telemetry || record.offset_ms === undefined) return charts;
  const from = record.offset_ms / 1000;
  const to = from + record.duration_ms / 1000;
  const specs = new Map(sensorCharts(telemetry.sensors).map((c) => [c.key, c]));
  for (const keys of CATEGORY_SENSORS[category] ?? []) {
    for (const key of keys) {
      const series = telemetry.series.find((s) => s.key === key);
      const spec = specs.get(key);
      const points = series?.points.filter(([t]) => t >= from && t <= to).map(([t, v]) => [t - from, v] as [number, number]);
      if (!series || !spec || !points || points.length < 2) continue;
      charts.push({ title: spec.title, unit: spec.unit, points, zero: spec.zero, minSpan: spec.minSpan, format: spec.format });
      break;
    }
  }
  return charts;
}

/** One line for the home screen, e.g. "Qwen3 0.6B writes 44.9 tokens per second". */
export function homeHeadline(result: RunResult): string | null {
  const speed = categorize(result).find((c) => c.def.id === "speed");
  const gen = speed?.groups
    .flatMap((g) => g.metrics.filter((m) => m.key.endsWith("/gen_tps")).map((m) => ({ model: g.title, value: m.value })))
    .sort((a, b) => b.value - a.value)[0];
  return gen ? `${gen.model} writes ${formatNumber(gen.value)} tokens per second` : null;
}

export function formatNumber(v: number) {
  return Number.isInteger(v) ? v.toLocaleString() : v.toLocaleString(undefined, { maximumFractionDigits: 1 });
}
