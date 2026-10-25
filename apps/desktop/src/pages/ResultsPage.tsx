import { useEffect, useState } from "react";
import { api, formatDate, formatDuration, type ModuleRecord, type ResultSummary, type RunResult } from "../api";
import { Banner, EmptyState, Icon, PageHeader } from "../components/ui";

export default function ResultsPage({
  openPath,
  onOpen,
  onRun,
}: {
  openPath: string | null;
  onOpen: (path: string | null) => void;
  onRun: () => void;
}) {
  return openPath ? (
    <ResultDetail path={openPath} onBack={() => onOpen(null)} />
  ) : (
    <ResultList onOpen={onOpen} onRun={onRun} />
  );
}

function ResultList({ onOpen, onRun }: { onOpen: (path: string) => void; onRun: () => void }) {
  const [results, setResults] = useState<ResultSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listResults().then(setResults).catch((e) => setError(String(e)));
  }, []);

  return (
    <div className="page">
      <PageHeader title="Results" subtitle="Every benchmark you've run on this device, newest first." />
      {error && <Banner error icon="error">{error}</Banner>}
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
            <button
              key={r.path}
              className="list-item"
              onClick={() => onOpen(r.path)}
            >
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

  return (
    <div className="page">
      <PageHeader
        title={result ? `${result.profile.name} benchmark` : "Result"}
        subtitle={
          result
            ? `${formatDate(result.started_at)} · took ${formatDuration(Date.parse(result.finished_at) - Date.parse(result.started_at))} · InferMark ${result.infermark_version}`
            : undefined
        }
        actions={
          <button className="btn text" onClick={onBack}>
            <Icon name="arrow_back" />
            All results
          </button>
        }
      />
      {error && <Banner error icon="error">{error}</Banner>}
      {result?.modules.map((m) => <ModuleCard key={m.module_id} record={m} name={names.get(m.module_id) ?? m.module_id} />)}
    </div>
  );
}

function ModuleCard({ record, name }: { record: ModuleRecord; name: string }) {
  const s = record.status;
  const chip =
    s.status === "completed" ? (
      <span className="chip success">
        <Icon name="check" />
        Done in {formatDuration(record.duration_ms)}
      </span>
    ) : s.status === "skipped" ? (
      <span className="chip">Skipped</span>
    ) : s.status === "failed" ? (
      <span className="chip danger">Failed</span>
    ) : (
      <span className="chip">Stopped</span>
    );

  return (
    <section className="card">
      <div className="row spread">
        <h2 className="title-medium">{name}</h2>
        {chip}
      </div>
      {s.status === "skipped" && <p className="body-medium muted">{s.reason}</p>}
      {s.status === "failed" && <p className="body-medium muted">{s.error}</p>}
      {record.output.metrics.length > 0 && (
        <div className="grid tight">
          {record.output.metrics.map((m) => (
            <div className="metric" key={m.key}>
              <span className="label-medium muted">{m.label}</span>
              <span>
                <span className="value">{formatValue(m.value)}</span>
                {m.unit && <span className="unit">{m.unit}</span>}
              </span>
            </div>
          ))}
        </div>
      )}
      {record.output.notes.map((n, i) => (
        <Banner key={i}>{n}</Banner>
      ))}
    </section>
  );
}

function formatValue(v: number) {
  return Number.isInteger(v) ? v.toLocaleString() : v.toLocaleString(undefined, { maximumFractionDigits: 1 });
}
