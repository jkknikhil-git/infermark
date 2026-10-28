// State of the benchmark in progress. Lives at the app level so a run keeps going (and stays
// visible) while the user browses other pages.
import { useEffect, useReducer } from "react";
import {
  listenToRuns,
  type LiveValue,
  type ModuleRecord,
  type PreflightCheck,
  type ResultSummary,
  type SensorInventory,
  type SensorReading,
} from "./api";

/** Points kept per live chart (10 minutes at one sample a second). */
export const LIVE_POINTS = 600;

export interface RunState {
  phase: "idle" | "running" | "finished" | "error";
  profileKey: string | null;
  current: { moduleId: string; index: number; total: number; fraction: number; message: string } | null;
  finished: ModuleRecord[];
  summary: ResultSummary | null;
  error: string | null;
  /** What looked off when the run started. */
  checks: PreflightCheck[];
  /** Which sensors this device has; null until the run reports it. */
  sensors: SensorInventory | null;
  /** Sensor history by channel key (`cpu.utilization`), x = seconds since the run started. */
  history: Record<string, [number, number][]>;
  /** Seconds since the run started, from the latest sensor sample. */
  t: number;
  /** The latest number a module reported (e.g. generation tok/s) and its history. */
  live: LiveValue | null;
  liveHistory: [number, number][];
}

export type RunAction =
  | { type: "start"; profileKey: string }
  | { type: "run-started"; sensors: SensorInventory; checks: PreflightCheck[] }
  | { type: "sensors"; t: number; readings: SensorReading[] }
  | { type: "started"; moduleId: string; index: number; total: number }
  | { type: "progress"; moduleId: string; fraction: number; message: string; live?: LiveValue }
  | { type: "module-done"; record: ModuleRecord }
  | { type: "finished"; summary: ResultSummary }
  | { type: "error"; message: string }
  | { type: "reset" };

export const initialRun: RunState = {
  phase: "idle",
  profileKey: null,
  current: null,
  finished: [],
  summary: null,
  error: null,
  checks: [],
  sensors: null,
  history: {},
  t: 0,
  live: null,
  liveHistory: [],
};

function push(points: [number, number][] | undefined, p: [number, number]): [number, number][] {
  const next = [...(points ?? []), p];
  return next.length > LIVE_POINTS ? next.slice(next.length - LIVE_POINTS) : next;
}

function reduce(s: RunState, a: RunAction): RunState {
  switch (a.type) {
    case "start":
      return { ...initialRun, phase: "running", profileKey: a.profileKey };
    case "run-started":
      return { ...s, sensors: a.sensors, checks: a.checks };
    case "sensors": {
      const history = { ...s.history };
      for (const r of a.readings) {
        if (r.value === null) continue;
        const key = `${r.source}.${r.kind}`;
        history[key] = push(history[key], [a.t, r.value]);
      }
      return { ...s, history, t: a.t };
    }
    case "started":
      return { ...s, current: { moduleId: a.moduleId, index: a.index, total: a.total, fraction: 0, message: "" } };
    case "progress": {
      if (!s.current || s.current.moduleId !== a.moduleId) return s;
      const current = { ...s.current, fraction: a.fraction, message: a.message };
      if (!a.live) return { ...s, current };
      // A new kind of number (another model, another test) starts a fresh chart.
      const same = s.live?.label === a.live.label;
      return {
        ...s,
        current,
        live: a.live,
        liveHistory: push(same ? s.liveHistory : [], [s.t, a.live.value]),
      };
    }
    case "module-done":
      return { ...s, finished: [...s.finished, a.record] };
    case "finished":
      return { ...s, phase: "finished", current: null, summary: a.summary };
    case "error":
      return { ...s, phase: "error", current: null, error: a.message };
    case "reset":
      return initialRun;
  }
}

export function useRunState() {
  const [state, dispatch] = useReducer(reduce, initialRun);
  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    listenToRuns({
      onEvent: (e) => {
        if (e.type === "run-started") dispatch({ type: "run-started", sensors: e.sensors, checks: e.checks ?? [] });
        else if (e.type === "sensors") dispatch({ type: "sensors", t: e.t, readings: e.readings });
        else if (e.type === "module-started") dispatch({ type: "started", moduleId: e.module_id, index: e.index, total: e.total });
        else if (e.type === "module-progress")
          dispatch({
            type: "progress",
            moduleId: e.module_id,
            fraction: e.progress.fraction,
            message: e.progress.message,
            live: e.progress.live,
          });
        else if (e.type === "module-finished") dispatch({ type: "module-done", record: e.record });
      },
      onFinished: (summary) => dispatch({ type: "finished", summary }),
      onError: (message) => dispatch({ type: "error", message }),
    }).then((unlisten) => (cancelled ? unlisten() : (off = unlisten)));
    return () => {
      cancelled = true;
      off?.();
    };
  }, []);
  return [state, dispatch] as const;
}

/** Overall progress 0..1 across all modules. */
export function overallProgress(s: RunState): number {
  if (s.phase === "finished") return 1;
  if (!s.current || s.current.total === 0) return 0;
  return (s.current.index + s.current.fraction) / s.current.total;
}
