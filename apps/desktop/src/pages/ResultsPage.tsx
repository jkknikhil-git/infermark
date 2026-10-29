import { useEffect, useState } from "react";
import { api, formatDate, formatDuration, type ModuleRecord, type ResultSummary, type RunResult } from "../api";
import { LineChart } from "../components/Chart";
import { Banner, Checks, EmptyState, Icon, PageHeader } from "../components/ui";
import { missingSensors, sensorCharts } from "../sensor-charts";
import { categorize, formatNumber, type CategoryView } from "../summary";

export default function ResultsPage({
  openPath,
  onOpen,
  onRun,
  onCompare,
}: {
  openPath: string | null;
  onOpen: (path: string | null) => void;
  onRun: () => void;
  onCompare: () => void;
}) {
  return openPath ? (
    <ResultDetail path={openPath} onBack={() => onOpen(null)} />
  ) : (
    <ResultList onOpen={onOpen} onRun={onRun} onCompare={onCompare} />
  );
}

function ResultList({ onOpen, onRun, onCompare }: { onOpen: (path: string) => void; onRun: () => void; onCompare: () => void }) {
  const [results, setResults] = useState<ResultSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listResults().then(setResults).catch((e) => setError(String(e)));
  }, []);

  return (
    <div className="page">
      <PageHeader
        title="Results"
        subtitle="Every benchmark you've run on this device, newest first."
        actions={
          results && results.length > 0 ? (
            <button className="btn text" onClick={onCompare}>
              <Icon name="compare_arrows" />
              Compare
            </button>
          ) : undefined
        }
      />
      {error && (
        <Banner error icon="error">
          {error}
        </Banner>
      )}
      {results && results.length === 0 && (
        <EmptyState
          icon="insights"
          title="No results yet"
          action={
            <button className="btn filled" onClick={onRun}>
              <Icon name="play_arrow" />
              Run your first benchmark
            </button>
          }
        >
          Your results will appear here. They're stored only on this device.
        </EmptyState>
      )}
      {results && results.length > 0 && (
        <div className="list card outlined" style={{ padding: "4px 16px" }}>
          {results.map((r) => (
            <button key={r.path} className="list-item" onClick={() => onOpen(r.path)}>
              <span className="avatar">
                <Icon name="speed" />
              </span>
              <span className="content">
                <span className="body-large">{r.profile_name}</span>
                <span className="body-medium muted">
                  {formatDate(r.started_at)} · {formatDuration(r.duration_ms)} · {r.completed} done
                  {r.skipped > 0 && `, ${r.skipped} skipped`}
                  {r.failed > 0 && `, ${r.failed} failed`}
                </span>
              </span>
              <Icon name="chevron_right" />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function ResultDetail({ path, onBack }: { path: string; onBack: () => void }) {
  const [result, setResult] = useState<RunResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.loadResult(path).then(setResult).catch((e) => setError(String(e)));
  }, [path]);

  const names = new Map(result?.plan.map((p) => [p.module_id, p.descriptor?.name ?? p.module_id]));
  const categories = result ? categorize(result) : [];

  return (
    <div className="page">
      <PageHeader
        onBack={onBack}
        title={result ? `${result.profile.name} benchmark` : "Result"}
        subtitle={
          result
            ? `${formatDate(result.started_at)} · took ${formatDuration(Date.parse(result.finished_at) - Date.parse(result.started_at))}`
            : undefined
        }
      />
      {error && (
        <Banner error icon="error">
          {error}
        </Banner>
      )}

      {result && (
        <>
          <section className="card primary score-pending" aria-label="Score">
            <span className="label-medium">INFERMARK SCORE</span>
            <span className="body-medium">
              Combines the categories below. It arrives once the endurance, battery and quality tests are in.
            </span>
          </section>

          <Checks checks={result.preflight ?? []} title="When this run started" />

          <section className="card" aria-label="Categories" style={{ padding: "8px 16px" }}>
            {categories.map((c) => (
              <CategoryRow key={c.def.id} view={c} />
            ))}
          </section>

          {result.telemetry && <SensorsCard telemetry={result.telemetry} />}

          <details className="card disclosure">
            <summary>
              <Icon name="list_alt" />
              <span className="title-medium" style={{ flex: 1 }}>
                Test log
              </span>
              <Icon name="expand_more" />
            </summary>
            <div className="list">
              {result.modules.map((m) => (
                <LogRow key={m.module_id} record={m} name={names.get(m.module_id) ?? m.module_id} />
              ))}
            </div>
            <p className="body-small muted">
              InferMark {result.infermark_version} · device {result.hardware.fingerprint}
            </p>
          </details>
        </>
      )}
    </div>
  );
}

function CategoryRow({ view }: { view: CategoryView }) {
  const measured = view.groups.length > 0;
  return (
    <details className="category" open={view.def.id === "speed" && measured ? true : undefined}>
      <summary aria-disabled={!measured}>
        <span className={measured ? "avatar" : "avatar muted-avatar"}>
          <Icon name={view.def.icon} />
        </span>
        <span className="content">
          <span className="body-large">{view.def.name}</span>
          <span className="body-medium muted">{measured ? view.headline : `Not measured: ${view.missing}`}</span>
        </span>
        {measured && <Icon name="expand_more" />}
      </summary>
      {measured && (
        <div className="category-body">
          <p className="body-small muted">{view.def.blurb}</p>
          {view.charts.length > 0 && (
            <div className="grid charts">
              {view.charts.map((c) => (
                <LineChart key={c.title} title={c.title} unit={c.unit} points={c.points} zero={c.zero} minSpan={c.minSpan} format={c.format} table />
              ))}
            </div>
          )}
          {view.groups.map((g) => (
            <div key={g.title} className="metric-group">
              <h3 className="title-small">{g.title}</h3>
              <div className="grid tight">
                {g.metrics.map((m) => (
                  <div className="metric" key={m.key}>
                    <span className="label-medium muted">{m.label}</span>
                    <span>
                      <span className="value">{formatNumber(m.value)}</span>
                      {m.unit && <span className="unit">{m.unit}</span>}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </details>
  );
}

/** Every sensor recorded during the run, plus the standard ones this device doesn't have. */
function SensorsCard({ telemetry }: { telemetry: NonNullable<RunResult["telemetry"]> }) {
  const charts = sensorCharts(telemetry.sensors);
  const missing = missingSensors(telemetry.sensors);
  const series = new Map(telemetry.series.map((s) => [s.key, s.points]));
  return (
    <details className="card disclosure">
      <summary>
        <Icon name="monitor_heart" />
        <span className="title-medium" style={{ flex: 1 }}>
          Sensors during the run
        </span>
        <Icon name="expand_more" />
      </summary>
      {charts.length > 0 ? (
        <div className="grid charts">
          {charts.map((c) => (
            <LineChart
              key={c.key}
              title={c.title}
              unit={c.unit}
              points={series.get(c.key) ?? []}
              zero={c.zero} minSpan={c.minSpan}
              format={c.format}
              empty={c.empty === "No readings yet" ? "No readings during this run" : c.empty}
              table
            />
          ))}
        </div>
      ) : (
        <p className="body-medium muted">This device didn't offer any sensors InferMark can read.</p>
      )}
      {missing && <p className="body-small muted">Not available on this device: {missing}.</p>}
    </details>
  );
}

function LogRow({ record, name }: { record: ModuleRecord; name: string }) {
  const s = record.status;
  const detail =
    s.status === "completed"
      ? `Done in ${formatDuration(record.duration_ms)}`
      : s.status === "skipped"
        ? `Skipped: ${s.reason}`
        : s.status === "failed"
          ? `Failed: ${s.error}`
          : "Stopped";
  return (
    <div className="list-item" style={{ alignItems: "flex-start" }}>
      <span className={s.status === "completed" ? "lead ok" : s.status === "failed" ? "lead bad" : "lead"}>
        <Icon name={s.status === "completed" ? "check_circle" : s.status === "failed" ? "error" : "do_not_disturb_on"} />
      </span>
      <div className="content">
        <span className="body-large">{name}</span>
        <span className="body-medium muted">{detail}</span>
        {record.output.notes.map((n, i) => (
          <span key={i} className="body-small muted">
            {n}
          </span>
        ))}
      </div>
    </div>
  );
}
