// Which sensor charts to draw, in what order, and how each is scaled. Shared by the live run
// screen and the results page so both read the same.

import type { SensorChannel, SensorInventory } from "./api";

export interface SensorChart {
  key: string;
  title: string;
  unit: string;
  /** Start the y-axis at 0. */
  zero: boolean;
  /** Smallest y-range drawn, in the chart's unit. */
  minSpan: number;
  /** Shown when the channel exists but produced no readings. */
  empty: string;
  /** Words for the headline value instead of a number (thermal state). */
  format?: (value: number) => string;
  /** Where readings turn from good to bad; charts without one stay neutral. */
  health?: Health;
}

/**
 * Fixed limits for a chart whose direction is clear: at or past `good` is good, at or past
 * `bad` is bad (either may be the larger number). `words` name the three bands; leave them out
 * when the value is already a word (thermal state).
 */
export interface Health {
  good: number;
  bad: number;
  words?: [string, string, string];
}

export type HealthLevel = "good" | "warning" | "bad";

const THERMAL = ["Nominal", "Fair", "Serious", "Critical"];
export const thermalLabel = (v: number) => THERMAL[Math.round(v)] ?? String(v);

const TEMPERATURE_WORDS: [string, string, string] = ["Cool", "Warm", "Hot"];

/** Limits per channel. CPU and GPU match the pre-run check, which warns at 80 °C. */
const HEALTH: Record<string, Health> = {
  "cpu.temperature": { good: 60, bad: 90, words: TEMPERATURE_WORDS },
  "gpu.temperature": { good: 60, bad: 90, words: TEMPERATURE_WORDS },
  "battery.temperature": { good: 35, bad: 45, words: ["Normal", "Warm", "Hot"] },
  "device.thermal-state": { good: 0, bad: 3 },
  "battery.battery-charge": { good: 50, bad: 15, words: ["OK", "Low", "Very low"] },
};

const DRAIN_WORDS: [string, string, string] = ["Light", "Moderate", "Heavy"];

/** Battery drain depends on the device class: 8 W is hard work for a phone, idle for a laptop. */
function drainHealth(os: string | null | undefined): Health | undefined {
  if (!os) return undefined;
  return os === "android" || os === "ios"
    ? { good: 3, bad: 8, words: DRAIN_WORDS }
    : { good: 15, bad: 40, words: DRAIN_WORDS };
}

/** Limits for a sensor channel (`gpu0.temperature` counts as a GPU), if it has a direction. */
export function healthFor(key: string, os?: string | null): Health | undefined {
  if (key === "battery.power") return drainHealth(os);
  return HEALTH[key.replace(/^([a-z]+)\d+\./, "$1.")];
}

/** Speed has no universal "good": compare it with the series' own peak, to show throttling. */
export function speedHealth(points: [number, number][]): Health | undefined {
  const peak = Math.max(0, ...points.map((p) => p[1]));
  if (peak <= 0) return undefined;
  // 90 %, not 95: back-to-back runs on a phone already spread by about 10 %.
  return { good: peak * 0.9, bad: peak * 0.6, words: ["Full speed", "Slowing", "Throttled"] };
}

/** 0 at or past the good limit, 1 at or past the bad one. */
export function badness(value: number, h: Health): number {
  return Math.min(1, Math.max(0, (value - h.good) / (h.bad - h.good || 1)));
}

/** Which band a value falls in, and its word when the health has words. */
export function statusOf(value: number, h: Health): { level: HealthLevel; word: string | null } {
  const t = (value - h.good) / (h.bad - h.good || 1);
  const level: HealthLevel = t <= 0 ? "good" : t >= 1 ? "bad" : "warning";
  const word = h.words ? h.words[level === "good" ? 0 : level === "warning" ? 1 : 2] : null;
  return { level, word };
}

const ORDER: SensorChannel["kind"][] = ["temperature", "thermal-state", "utilization", "power", "clock", "battery-charge", "memory-used"];
const MIN_SPAN: Record<SensorChannel["kind"], number> = {
  temperature: 10,
  "thermal-state": 3,
  utilization: 100,
  power: 5,
  clock: 500,
  "battery-charge": 5,
  "memory-used": 1024 ** 3,
};
const UNITS: Record<SensorChannel["kind"], string> = {
  temperature: "°C",
  "thermal-state": "",
  utilization: "%",
  power: "W",
  clock: "MHz",
  "battery-charge": "%",
  "memory-used": "bytes",
};

function rank(c: SensorChannel) {
  const family = c.source.replace(/\d+$/, "");
  const source = ["cpu", "gpu", "device", "battery", "system"].indexOf(family);
  return (source < 0 ? 9 : source) * 10 + ORDER.indexOf(c.kind);
}

export function sensorCharts(inventory: SensorInventory, os?: string | null): SensorChart[] {
  return [...inventory.available]
    .sort((a, b) => rank(a) - rank(b))
    .map((c) => ({
      key: `${c.source}.${c.kind}`,
      title: c.label,
      unit: UNITS[c.kind],
      zero: c.kind === "utilization" || c.kind === "power" || c.kind === "thermal-state",
      minSpan: MIN_SPAN[c.kind],
      empty:
        c.source === "battery" && c.kind === "power"
          ? "Only measured while running on battery"
          : "No readings yet",
      format: c.kind === "thermal-state" ? thermalLabel : undefined,
      health: healthFor(`${c.source}.${c.kind}`, os),
    }));
}

/** "CPU temperature, GPU load and Thermal state", or null when nothing is missing. */
export function missingSensors(inventory: SensorInventory): string | null {
  // "Thermal state" -> "thermal state" mid-sentence; "CPU load" keeps its capitals.
  const labels = inventory.missing.map((c, i) => (i > 0 && !/^[A-Z]{2}/.test(c.label) ? c.label.charAt(0).toLowerCase() + c.label.slice(1) : c.label));
  if (labels.length === 0) return null;
  if (labels.length === 1) return labels[0];
  return `${labels.slice(0, -1).join(", ")} and ${labels[labels.length - 1]}`;
}
