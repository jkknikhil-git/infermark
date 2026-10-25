// State of the benchmark in progress. Lives at the app level so a run keeps going (and stays
// visible) while the user browses other pages.
import { useEffect, useReducer } from "react";
import { listenToRuns, type ModuleRecord, type ResultSummary } from "./api";

export interface RunState {
  phase: "idle" | "running" | "finished" | "error";
  profileKey: string | null;
  current: { moduleId: string; index: number; total: number; fraction: number; message: string } | null;
  finished: ModuleRecord[];
  summary: ResultSummary | null;
  error: string | null;
}

export type RunAction =
  | { type: "start"; profileKey: string }
  | { type: "started"; moduleId: string; index: number; total: number }
  | { type: "progress"; moduleId: string; fraction: number; message: string }
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
};

function reduce(s: RunState, a: RunAction): RunState {
  switch (a.type) {
    case "start":
      return { ...initialRun, phase: "running", profileKey: a.profileKey };
    case "started":
      return { ...s, current: { moduleId: a.moduleId, index: a.index, total: a.total, fraction: 0, message: "" } };
    case "progress":
      return s.current && s.current.moduleId === a.moduleId
        ? { ...s, current: { ...s.current, fraction: a.fraction, message: a.message } }
        : s;
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
        if (e.type === "module-started") dispatch({ type: "started", moduleId: e.module_id, index: e.index, total: e.total });
        else if (e.type === "module-progress")
          dispatch({ type: "progress", moduleId: e.module_id, fraction: e.progress.fraction, message: e.progress.message });
        else dispatch({ type: "module-done", record: e.record });
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
