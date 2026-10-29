import { useCallback, useEffect, useState } from "react";
import {
  api,
  listenToDownloads,
  size,
  type DownloadProgress,
  type EngineView,
  type ModelEntry,
  type ModelsView,
  type Tier,
} from "../api";
import { Banner, Icon, PageHeader, Progress } from "../components/ui";

const TIER: Record<Tier, { label: string; chip: string; icon: string }> = {
  "full-gpu": { label: "Full GPU", chip: "success", icon: "bolt" },
  "partial-offload": { label: "Partial GPU", chip: "success", icon: "splitscreen" },
  "system-memory": { label: "Fits", chip: "success", icon: "check_circle" },
  "too-slow": { label: "Too slow", chip: "warn", icon: "hourglass_bottom" },
  "too-big": { label: "Too big", chip: "danger", icon: "block" },
};

const fits = (m: ModelEntry) => m.verdict.tier !== "too-big" && m.verdict.tier !== "too-slow";

export default function ModelsPage({ onOpenSettings }: { onOpenSettings: () => void }) {
  const [view, setView] = useState<ModelsView | null>(null);
  const [engine, setEngine] = useState<EngineView | null>(null);
  const [progress, setProgress] = useState<Record<string, DownloadProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api.listModels().then(setView, (e) => setError(String(e)));
    api.engineStatus().then(setEngine, (e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    const off = listenToDownloads({
      onProgress: (p) => setProgress((all) => ({ ...all, [p.id]: p })),
      onDone: (d) => {
        setProgress(({ [d.id]: _, ...rest }) => rest);
        if (d.error && d.error !== "cancelled") setError(d.error);
        refresh();
      },
    });
    return () => {
      off.then((f) => f());
    };
  }, [refresh]);

  const run = (p: Promise<unknown>) => p.catch((e) => setError(String(e)));

  const download = (id: string) => {
    setError(null);
    setProgress((all) => ({ ...all, [id]: { id, phase: "downloading", done: 0, total: 1 } }));
    run(api.downloadModel(id));
  };

  const remove = (id: string) => {
    setConfirmDelete(null);
    run(api.deleteModel(id).then(refresh));
  };

  const models = view?.models.filter((m) => showAll || fits(m) || m.installed) ?? [];
  const hidden = (view?.models.length ?? 0) - models.length;
  const llama = engine?.status.llama_cpp;
  const builtIn = engine?.status.built_in;
  const recommended = engine?.builds.find((b) => b.recommended);
  const engineProgress = progress["engine"];

  return (
    <div className="page">
      <PageHeader
        title="Models"
        subtitle="Open models you can benchmark, checked against this device before you download anything."
        actions={
          <button className="btn text" onClick={refresh}>
            <Icon name="refresh" />
            Refresh
          </button>
        }
      />

      {error && (
        <Banner error icon="error">
          {error}
        </Banner>
      )}

      <section className="card" aria-label="Inference engine">
        <div className="card-head">
          <span className="avatar">
            <Icon name="memory" />
          </span>
          <h2 className="title-medium" style={{ flex: 1 }}>
            Inference engine
          </h2>
          {(llama || builtIn) && (
            <span className="chip success">
              <Icon name="check" />
              Ready
            </span>
          )}
        </div>
        {builtIn && !llama ? (
          <dl className="kv body-medium">
            <dt>llama.cpp</dt>
            <dd>{builtIn} · built into this app, runs on the CPU</dd>
          </dl>
        ) : llama ? (
          <dl className="kv body-medium">
            <dt>llama.cpp</dt>
            <dd>
              {llama.version}
              {llama.accel ? ` · ${engine?.builds.find((b) => b.accel === llama.accel)?.accel_label ?? llama.accel}` : " · custom build"}
            </dd>
          </dl>
        ) : (
          <>
            <p className="body-medium muted">
              InferMark runs models with llama.cpp, the open-source engine behind most local AI apps. It installs the official
              build for your hardware, checked against its published fingerprint.
            </p>
            {engineProgress ? (
              <DownloadBar p={engineProgress} onCancel={() => run(api.cancelDownload("engine"))} />
            ) : (
              recommended && (
                <div className="row">
                  <button className="btn filled" onClick={() => run(api.installEngine(recommended.id))}>
                    <Icon name="download" />
                    Install llama.cpp
                  </button>
                  <span className="body-medium muted">
                    {recommended.accel_label} · {size(recommended.download_bytes)}
                  </span>
                </div>
              )
            )}
          </>
        )}
        <hr className="divider" />
        <dl className="kv body-medium">
          <dt>Ollama</dt>
          <dd>
            {engine?.status.ollama
              ? `${engine.status.ollama.version} running · ${engine.status.ollama.models.length} models`
              : "Not running (optional)"}
          </dd>
        </dl>
      </section>

      {view && (
        <div className="row spread">
          <button className="btn text" onClick={onOpenSettings} title="Change where models are saved">
            <Icon name="folder" />
            <span className="mono" style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
              {view.models_dir}
            </span>
          </button>
          <span className="body-medium muted">{view.free_bytes != null && `${size(view.free_bytes)} free`}</span>
        </div>
      )}

      {view && !view.speed_known && (
        <Banner icon="speed">Run a benchmark once and InferMark will also predict how fast each model will be here.</Banner>
      )}

      <section className="card" aria-label="Models">
        <div className="row spread">
          <h2 className="title-medium">Catalog</h2>
          <div className="segmented" role="group" aria-label="Filter models">
            <button aria-pressed={!showAll} onClick={() => setShowAll(false)}>
              Fits this device
            </button>
            <button aria-pressed={showAll} onClick={() => setShowAll(true)}>
              All
            </button>
          </div>
        </div>
        <div className="list">
          {models.map((m) => {
            const p = progress[m.id];
            const tier = TIER[m.verdict.tier];
            const tps = m.verdict.predicted_tokens_per_sec;
            return (
              <div className="list-item model-item" key={m.id}>
                <span className={fits(m) ? "lead ok" : "lead bad"}>
                  <Icon name={m.installed ? "download_done" : "deployed_code"} />
                </span>
                <div className="content">
                  <span className="body-large">{m.name}</span>
                  <span className="body-small muted">
                    {m.params_b}B · {m.quant} · {size(m.size_bytes)} · {m.license}
                  </span>
                  <span className="body-small muted">{m.verdict.summary}</span>
                  {p && <DownloadBar p={p} onCancel={() => run(api.cancelDownload(m.id))} />}
                </div>
                <div className="model-actions">
                  <span className={`chip ${tier.chip}`} title={m.verdict.summary}>
                    <Icon name={tier.icon} />
                    {tier.label}
                    {tps != null && fits(m) && ` · ~${tps < 10 ? tps.toFixed(1) : Math.round(tps)} tok/s`}
                  </span>
                  {!p &&
                    (m.installed ? (
                      confirmDelete === m.id ? (
                        <button className="btn text" onClick={() => remove(m.id)}>
                          <Icon name="delete" />
                          Delete {size(m.size_bytes)}?
                        </button>
                      ) : (
                        <button className="icon-btn" aria-label={`Delete ${m.name}`} title="Delete" onClick={() => setConfirmDelete(m.id)}>
                          <Icon name="delete" />
                        </button>
                      )
                    ) : (
                      m.verdict.tier !== "too-big" && (
                        <button
                          className={fits(m) ? "btn tonal" : "btn outlined"}
                          onClick={() => download(m.id)}
                          title={m.partial_bytes > 0 ? "Resume download" : `Download ${size(m.size_bytes)}`}
                        >
                          <Icon name="download" />
                          {m.partial_bytes > 0 ? "Resume" : fits(m) ? "Download" : "Download anyway"}
                        </button>
                      )
                    ))}
                </div>
              </div>
            );
          })}
        </div>
        {hidden > 0 && !showAll && (
          <button className="btn text" onClick={() => setShowAll(true)}>
            Show {hidden} model{hidden > 1 ? "s" : ""} that won't run well here
          </button>
        )}
      </section>
    </div>
  );
}

function DownloadBar({ p, onCancel }: { p: DownloadProgress; onCancel: () => void }) {
  const fraction = p.total > 0 ? p.done / p.total : 0;
  return (
    <div className="row" style={{ gap: 12, marginTop: 6, flexWrap: "nowrap" }}>
      <div style={{ flex: 1 }}>
        <Progress value={fraction} />
      </div>
      <span className="body-small muted" style={{ minWidth: 150, textAlign: "right" }}>
        {p.phase === "verifying" ? "Checking file" : "Downloading"} {Math.round(fraction * 100)}% · {size(p.done)} of{" "}
        {size(p.total)}
      </span>
      {p.phase === "downloading" && (
        <button className="icon-btn" aria-label="Stop download" title="Stop (resumes later)" onClick={onCancel}>
          <Icon name="close" />
        </button>
      )}
    </div>
  );
}
