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
}

const THERMAL = ["Nominal", "Fair", "Serious", "Critical"];
export const thermalLabel = (v: number) => THERMAL[Math.round(v)] ?? String(v);

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

export function sensorCharts(inventory: SensorInventory): SensorChart[] {
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
