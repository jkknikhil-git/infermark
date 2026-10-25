import { useEffect, useState, type Dispatch } from "react";
import { api, formatDuration, type PlanEntry, type ProfileEntry } from "../api";
import { Banner, Icon, PageHeader, Progress } from "../components/ui";
import { overallProgress, type RunAction, type RunState } from "../run-state";

export default function RunPage({
  run,
  dispatch,
  onShowResult,
}: {
  run: RunState;
  dispatch: Dispatch<RunAction>;
  onShowResult: (path: string) => void;
}) {
  const [profiles, setProfiles] = useState<ProfileEntry[]>([]);
  const [selected, setSelected] = useState<string>(run.profileKey ?? "quick");
  const [plan, setPlan] = useState<PlanEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listProfiles().then((l) => setProfiles(l.profiles)).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    setPlan(null);
    api.planProfile(selected).then(setPlan).catch((e) => setError(String(e)));
  }, [selected]);

  const start = async () => {
    setError(null);
    dispatch({ type: "start", profileKey: selected });
    try {
      await api.startRun(selected);
    } catch (e) {
      dispatch({ type: "error", message: String(e) });
    }
  };

  const running = run.phase === "running";
  const planned = plan?.filter((p) => p.status.status === "planned") ?? [];
  const seconds = planned.reduce((t, p) => t + (p.descriptor?.estimated_seconds ?? 0), 0);

  return (
    <div className="page">
      <PageHeader title="Run a benchmark" subtitle="Pick how thorough you want it to be. You can keep using the app while it runs." />

      {error && <Banner error icon="error">{error}</Banner>}

      {run.phase !== "idle" && (
        <section className="card outlined" aria-live="polite">
          <div className="row spread">
            <div className="card-head">
              <span className="avatar">
                <Icon name={run.phase === "finished" ? "check" : run.phase === "error" ? "error" : "speed"} />
              </span>
              <div>
                <h2 className="title-medium">
                  {run.phase === "running" && "Benchmark running"}
                  {run.phase === "finished" && "Benchmark complete"}
                  {run.phase === "error" && "Benchmark stopped"}
                </h2>
                <p className="body-medium muted">
                  {run.phase === "running" &&
                    (run.current
                      ? `Test ${run.current.index + 1} of ${run.current.total}: ${nameOf(plan, run.current.moduleId)}${run.current.message ? ` · ${run.current.message}` : ""}`
                      : "Starting…")}
                  {run.phase === "finished" && run.summary && `${run.summary.completed} tests done in ${formatDuration(run.summary.duration_ms)}`}
                  {run.phase === "error" && run.error}
                </p>
              </div>
            </div>
            <div className="row">
              {running && (
                <button className="btn outlined" onClick={() => api.cancelRun()}>
                  <Icon name="stop" />
                  Stop
                </button>
              )}
              {run.phase === "finished" && run.summary && (
                <button className="btn filled" onClick={() => onShowResult(run.summary!.path)}>
                  <Icon name="insights" />
                  View results
                </button>
              )}
              {!running && (
                <button className="btn text" onClick={() => dispatch({ type: "reset" })}>
                  Dismiss
                </button>
              )}
            </div>
          </div>
          {(running || run.phase === "finished") && <Progress value={overallProgress(run)} />}
        </section>
      )}

      <section aria-label="Choose a profile" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <h2 className="title-medium">1. Choose a profile</h2>
        <div className="grid" role="radiogroup">
          {profiles.map((p) => {
            const enabled = Object.values(p.profile.modules).filter((m) => m.enabled !== false).length;
            return (
              <button
                key={p.key}
                className="choice"
                role="radio"
                aria-checked={selected === p.key}
                disabled={running}
                onClick={() => setSelected(p.key)}
              >
                <span className="row spread">
                  <span className="title-medium">{p.profile.name}</span>
                  {selected === p.key && <Icon name="check_circle" filled />}
                </span>
                <span className="body-medium muted">{p.profile.description}</span>
                <span className="row" style={{ gap: 8 }}>
                  <span className="chip">{enabled} tests</span>
                  <span className="chip">{p.source.kind === "builtin" ? "Built-in" : "Yours"}</span>
                </span>
              </button>
            );
          })}
        </div>
      </section>

      <section className="card" aria-label="What will run">
        <div className="row spread">
          <h2 className="title-medium">2. What will run on this device</h2>
          {plan && (
            <span className="body-medium muted">
              {planned.length} of {plan.length} tests{seconds > 0 && ` · about ${formatDuration(seconds * 1000)}`}
            </span>
          )}
        </div>
        {!plan && <p className="body-medium muted">Checking your hardware…</p>}
        {plan && (
          <div className="list">
            {plan.map((entry) => {
              const ok = entry.status.status === "planned";
              return (
                <div className="list-item" key={entry.module_id}>
                  <span className={ok ? "lead ok" : "lead"}>
                    <Icon name={ok ? "check_circle" : "do_not_disturb_on"} filled={ok} />
                  </span>
                  <div className="content">
                    <span className="body-large">{entry.descriptor?.name ?? entry.module_id}</span>
                    <span className="body-medium muted">
                      {entry.status.status === "skipped"
                        ? `Skipped: ${entry.status.reason}`
                        : entry.descriptor?.description}
                    </span>
                  </div>
                  {ok && entry.descriptor && (
                    <span className="body-small muted">~{formatDuration(entry.descriptor.estimated_seconds * 1000)}</span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      <div className="sticky-actions row">
        <button className="btn filled" onClick={start} disabled={running || planned.length === 0}>
          <Icon name="play_arrow" />
          Start benchmark
        </button>
        <span className="body-small muted">Tip: plug in your charger and close other apps for the most accurate results.</span>
      </div>
    </div>
  );
}

function nameOf(plan: PlanEntry[] | null, id: string) {
  return plan?.find((p) => p.module_id === id)?.descriptor?.name ?? id;
}
