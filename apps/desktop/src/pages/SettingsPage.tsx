import { useEffect, useState } from "react";
import { api, type AppInfo, type SettingKey, type SettingsView } from "../api";
import { Banner, Icon, PageHeader } from "../components/ui";
import { ACCENTS, type Preferences, type ThemeMode } from "../theme";

const MODES: { id: ThemeMode; label: string; icon: string }[] = [
  { id: "system", label: "System", icon: "brightness_auto" },
  { id: "light", label: "Light", icon: "light_mode" },
  { id: "dark", label: "Dark", icon: "dark_mode" },
];

export default function SettingsPage({ prefs, onChange }: { prefs: Preferences; onChange: (p: Preferences) => void }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo);
  }, []);

  return (
    <div className="page">
      <PageHeader title="Settings" />

      <section className="card">
        <h2 className="title-medium">Appearance</h2>
        <div className="row spread">
          <span className="body-large">Theme</span>
          <div className="segmented" role="group" aria-label="Theme">
            {MODES.map((m) => (
              <button key={m.id} aria-pressed={prefs.mode === m.id} onClick={() => onChange({ ...prefs, mode: m.id })}>
                <Icon name={m.icon} small />
                {m.label}
              </button>
            ))}
          </div>
        </div>
        <hr className="divider" />
        <div className="row spread">
          <span className="body-large">Accent color</span>
          <div className="row" role="radiogroup" aria-label="Accent color" style={{ gap: 8 }}>
            {ACCENTS.map((a) => (
              <button
                key={a.hex}
                className="swatch"
                role="radio"
                aria-checked={prefs.accent === a.hex}
                aria-label={a.name}
                title={a.name}
                style={{ background: a.hex }}
                onClick={() => onChange({ ...prefs, accent: a.hex })}
              >
                {prefs.accent === a.hex && <Icon name="check" small />}
              </button>
            ))}
          </div>
        </div>
      </section>

      <StorageCard />

      <section className="card">
        <h2 className="title-medium">Your data</h2>
        <p className="body-medium muted">
          Everything InferMark saves stays in these folders on this device. Add your own profiles and test packs here.
        </p>
        {info && (
          <dl className="kv body-medium">
            <dt>Profiles</dt>
            <dd className="mono">{info.profiles_dir}</dd>
            <dt>Test packs</dt>
            <dd className="mono">{info.packs_dir}</dd>
            <dt>Results</dt>
            <dd className="mono">{info.results_dir}</dd>
          </dl>
        )}
      </section>

      <section className="card">
        <h2 className="title-medium">Privacy</h2>
        <div className="list">
          {[
            ["cloud_off", "No telemetry or analytics", "InferMark never phones home."],
            ["person_off", "No account needed", "Results contain no names, usernames or file paths."],
            ["download", "Network only when you ask", "Used only to download models and the engine you choose, each checked against its fingerprint."],
          ].map(([icon, title, body]) => (
            <div className="list-item" key={title}>
              <span className="lead ok">
                <Icon name={icon} />
              </span>
              <div className="content">
                <span className="body-large">{title}</span>
                <span className="body-medium muted">{body}</span>
              </div>
            </div>
          ))}
        </div>
      </section>

      <section className="card">
        <h2 className="title-medium">About</h2>
        <dl className="kv body-medium">
          <dt>Version</dt>
          <dd>{info?.version ?? "…"}</dd>
          <dt>License</dt>
          <dd>Apache-2.0 (open source)</dd>
          <dt>Source code</dt>
          <dd className="mono">github.com/jkknikhil-git/infermark</dd>
        </dl>
      </section>
    </div>
  );
}

/** Where models and engines are stored, plus the engine addresses. */
function StorageCard() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [draft, setDraft] = useState<Record<SettingKey, string>>({ storage_dir: "", llama_server: "", ollama_url: "" });
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<SettingKey | null>(null);

  const load = (v: SettingsView) => {
    setView(v);
    setDraft({
      storage_dir: v.settings.storage_dir ?? "",
      llama_server: v.settings.llama_server ?? "",
      ollama_url: v.settings.ollama_url,
    });
  };

  useEffect(() => {
    api.getSettings().then(load, (e) => setError(String(e)));
  }, []);

  const save = (key: SettingKey) => {
    setError(null);
    api.setSetting(key, draft[key]).then(
      (v) => {
        load(v);
        setSaved(key);
        window.setTimeout(() => setSaved(null), 2000);
      },
      (e) => setError(String(e)),
    );
  };

  const field = (key: SettingKey, label: string, help: string, placeholder: string) => {
    const current =
      key === "ollama_url" ? view?.settings.ollama_url ?? "" : (view?.settings[key] as string | undefined) ?? "";
    const dirty = draft[key] !== current;
    return (
      <div className="field">
        <label className="label-large" htmlFor={`setting-${key}`}>
          {label}
        </label>
        <div className="row" style={{ flexWrap: "nowrap" }}>
          <input
            id={`setting-${key}`}
            className="text-input mono"
            value={draft[key]}
            placeholder={placeholder}
            spellCheck={false}
            onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
            onKeyDown={(e) => e.key === "Enter" && dirty && save(key)}
          />
          <button className="btn tonal" disabled={!dirty} onClick={() => save(key)}>
            {saved === key ? <Icon name="check" /> : null}
            {saved === key ? "Saved" : "Save"}
          </button>
        </div>
        <span className="body-small muted">{help}</span>
      </div>
    );
  };

  return (
    <section className="card">
      <h2 className="title-medium">Storage and engines</h2>
      {error && (
        <Banner error icon="error">
          {error}
        </Banner>
      )}
      {field(
        "storage_dir",
        "Storage folder",
        `Downloaded models and the llama.cpp engine go here (they can take many GB). Leave empty for the default. Files already downloaded are not moved.`,
        view?.default_storage_dir ?? "",
      )}
      {field(
        "ollama_url",
        "Ollama address",
        "Only used if you benchmark models you already have in Ollama.",
        "http://127.0.0.1:11434",
      )}
      {field(
        "llama_server",
        "Custom llama-server (advanced)",
        "Use your own llama.cpp build instead of the one InferMark installs. Leave empty to use the installed one.",
        "path to llama-server",
      )}
    </section>
  );
}
