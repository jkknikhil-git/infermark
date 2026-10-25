import { useEffect, useState } from "react";
import { api, type AppInfo } from "../api";
import { Icon, PageHeader } from "../components/ui";
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
            ["download", "Network only when you ask", "Used only to download models you choose."],
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
