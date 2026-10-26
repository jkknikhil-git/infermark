import { useEffect, useState, type Dispatch } from "react";
import { api, formatDuration, type PlanEntry, type ProfileEntry } from "../api";
import { Banner, Icon, PageHeader, Progress } from "../components/ui";
import { overallProgress, type RunAction, type RunState } from "../run-state";

/**
 * Full-screen run flow (opened from Home). Before a run: pick a profile and see what will run
 * on this device. During a run: live progress, big and readable on a phone.
 */
export default function RunPage({
  run,
  dispatch,
  profileKey,
  onProfileChange,
  onStart,
  backgrounded,
  onShowResult,
  onClose,
}: {
  run: RunState;
  dispatch: Dispatch<RunAction>;
  profileKey: string;
  onProfileChange: (key: string) => void;
  onStart: (key: string) => void;
  /** The app went to the background during this run. */
  backgrounded: boolean;
  onShowResult: (path: string) => void;
  onClose: () => void;
}) {
  const [profiles, setProfiles] = useState<ProfileEntry[]>([]);
  const [plan, setPlan] = useState<PlanEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listProfiles().then((l) => setProfiles(l.profiles)).catch((e) => setError(String(e)));
  }, []);

  const selected = run.profileKey ?? profileKey;
  useEffect(() => {
    setPlan(null);
    api.planProfile(selected).then(setPlan).catch((e) => setError(String(e)));
  }, [selected]);

  const running = run.phase === "running";
  const planned = plan?.filter((p) => p.status.status === "planned") ?? [];
  const seconds = planned.reduce((t, p) => t + (p.descriptor?.estimated_seconds ?? 0), 0);
  const doneIds = new Set(run.finished.map((r) => r.module_id));

  return (
    <div className="page">
      <PageHeader
        onBack={onClose}
        title={running ? "Benchmark running" : run.phase === "finished" ? "Benchmark complete" : "Run a benchmark"}
        subtitle={
          run.phase === "idle"
            ? "Pick how thorough you want it to be."
            : profiles.find((p) => p.key === selected)?.profile.name
        }
      />

      {error && (
        <Banner error icon="error">
          {error}
        </Banner>
      )}

      {run.phase !== "idle" && (
        <section className="card primary live-run" aria-live="polite">
          <span className="display-medium">{Math.round(overallProgress(run) * 100)}%</span>
          <span className="title-medium">
            {running &&
              (run.current
                ? `Test ${run.current.index + 1} of ${run.current.total}: ${nameOf(plan, run.current.moduleId)}`
                : "Starting…")}
            {run.phase === "finished" && run.summary && `${run.summary.completed} tests done in ${formatDuration(run.summary.duration_ms)}`}
            {run.phase === "error" && "Benchmark stopped"}
          </span>
          {running && run.current?.message && <span className="body-medium">{run.current.message}</span>}
          {run.phase === "error" && <span className="body-medium">{run.error}</span>}
          {(running || run.phase === "finished") && <Progress value={overallProgress(run)} />}
          <div className="row">
            {running && (
              <button className="btn outlined on-container" onClick={() => api.cancelRun()}>
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
              <button className="btn text on-container" onClick={() => dispatch({ type: "reset" })}>
                New run
              </button>
            )}
          </div>
        </section>
      )}

      {running && (
        <Banner icon="screen_lock_portrait">
          Keep InferMark open while it runs. The screen stays on, and leaving the app can slow the test down.
        </Banner>
      )}
      {backgrounded && run.phase !== "idle" && (
        <Banner icon="warning">
          InferMark was in the background during this run, so some results may be lower than this device can really do.
        </Banner>
      )}

      {run.phase === "idle" && (
        <section aria-label="Choose a profile" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <h2 className="title-medium">Profile</h2>
          <div className="grid" role="radiogroup">
            {profiles.map((p) => {
              const enabled = Object.values(p.profile.modules).filter((m) => m.enabled !== false).length;
              return (
                <button
                  key={p.key}
                  className="choice"
                  role="radio"
                  aria-checked={selected === p.key}
                  onClick={() => onProfileChange(p.key)}
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
      )}

      <section className="card" aria-label="Tests">
        <div className="row spread">
          <h2 className="title-medium">{run.phase === "idle" ? "What will run on this device" : "Tests"}</h2>
          {plan && run.phase === "idle" && (
            <span className="body-medium muted">
              {planned.length} of {plan.length}
              {seconds > 0 && ` · about ${formatDuration(seconds * 1000)}`}
            </span>
          )}
        </div>
        {!plan && <p className="body-medium muted">Checking your hardware…</p>}
        {plan && (
          <div className="list">
            {plan.map((entry) => {
              const ok = entry.status.status === "planned";
              const active = running && run.current?.moduleId === entry.module_id;
              const done = doneIds.has(entry.module_id);
              const icon = active ? "progress_activity" : done ? "check_circle" : ok ? "radio_button_unchecked" : "do_not_disturb_on";
              return (
                <div className="list-item" key={entry.module_id}>
                  <span className={ok ? "lead ok" : "lead"}>
                    {active ? (
                      <span className="spin">
                        <Icon name={icon} />
                      </span>
                    ) : (
                      <Icon name={run.phase === "idle" && ok ? "check_circle" : icon} filled={done || (run.phase === "idle" && ok)} />
                    )}
                  </span>
                  <div className="content">
                    <span className="body-large">{entry.descriptor?.name ?? humanize(entry.module_id)}</span>
                    <span className="body-medium muted">
                      {entry.status.status === "skipped"
                        ? `Skipped: ${entry.status.reason}`
                        : active && run.current?.message
                          ? run.current.message
                          : entry.descriptor?.description}
                    </span>
                  </div>
                  {ok && entry.descriptor && run.phase === "idle" && (
                    <span className="body-small muted">~{formatDuration(entry.descriptor.estimated_seconds * 1000)}</span>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>

      {run.phase === "idle" && (
        <div className="sticky-actions">
          <button className="btn filled large" onClick={() => onStart(selected)} disabled={planned.length === 0}>
            <Icon name="play_arrow" />
            Start benchmark
          </button>
          <span className="body-small muted">Tip: plug in your charger and close other apps for the most accurate results.</span>
        </div>
      )}
    </div>
  );
}

function nameOf(plan: PlanEntry[] | null, id: string) {
  return plan?.find((p) => p.module_id === id)?.descriptor?.name ?? humanize(id);
}

/** `quality-packs` -> `Quality packs`, for modules this build doesn't have yet. */
function humanize(id: string) {
  const s = id.replace(/-/g, " ");
  return s.charAt(0).toUpperCase() + s.slice(1);
}
