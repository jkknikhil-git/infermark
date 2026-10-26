import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, gb, type HardwareInfo, type ProfileEntry, type RunResult } from "../api";
import { Banner, Icon, Progress } from "../components/ui";
import { overallProgress, type RunState } from "../run-state";
import { homeHeadline } from "../summary";

const LIVE_INTERVAL_MS = 2000;

const OS_LABEL: Record<string, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
};

/**
 * Home: like a phone benchmark app, the score and one big Start button come first; the device
 * summary sits below, with full details behind "See all".
 */
export default function HomePage({
  run,
  profileKey,
  onProfileChange,
  onStart,
  onOpenRun,
  onShowResult,
}: {
  run: RunState;
  profileKey: string;
  onProfileChange: (key: string) => void;
  onStart: (key: string) => void;
  onOpenRun: () => void;
  onShowResult: (path: string) => void;
}) {
  const [hw, setHw] = useState<HardwareInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [profiles, setProfiles] = useState<ProfileEntry[]>([]);
  const [latest, setLatest] = useState<{ path: string; result: RunResult } | null>(null);
  const [showAll, setShowAll] = useState(false);

  const refresh = useCallback(() => {
    setLoading(true);
    api
      .probe()
      .then((h) => {
        setHw(h);
        setError(null);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(refresh, [refresh]);

  useEffect(() => {
    api.listProfiles().then((l) => setProfiles(l.profiles), () => {});
  }, []);

  // The newest result feeds the headline under the score.
  useEffect(() => {
    if (run.phase === "running") return;
    api
      .listResults()
      .then((list) => (list[0] ? api.loadResult(list[0].path).then((result) => setLatest({ path: list[0].path, result })) : null))
      .catch(() => {});
  }, [run.phase]);

  // Memory and power change all the time (apps opening, charger unplugged), so keep them live.
  // Paused while the window is hidden to avoid wasting battery.
  useEffect(() => {
    let busy = false;
    const tick = () => {
      if (busy || document.hidden) return;
      busy = true;
      api
        .liveStatus()
        .then((live) => setHw((h) => (h ? { ...h, ...live } : h)))
        .catch(() => {
          /* keep the last known values */
        })
        .finally(() => (busy = false));
    };
    const id = window.setInterval(tick, LIVE_INTERVAL_MS);
    document.addEventListener("visibilitychange", tick);
    return () => {
      window.clearInterval(id);
      document.removeEventListener("visibilitychange", tick);
    };
  }, []);

  const running = run.phase === "running";
  const headline = latest ? homeHeadline(latest.result) : null;

  return (
    <div className="page">
      {error && (
        <Banner error icon="error">
          Couldn't read your hardware: {error}
        </Banner>
      )}

      <section className="card primary hero" aria-label="Benchmark">
        <div className="hero-device">
          <span className="label-medium">THIS DEVICE</span>
          <h1 className="title-large">{hw ? shortCpu(hw.cpu.brand) : loading ? "Checking…" : "Unknown device"}</h1>
          {hw && (
            <span className="body-medium">
              {gb(hw.memory.total_bytes, 0)} RAM · {hw.gpus[0] ? cleanName(hw.gpus[0].name) : "No GPU"} ·{" "}
              {OS_LABEL[hw.os.family] ?? hw.os.name}
            </span>
          )}
        </div>

        <div className="hero-score">
          <span className="label-medium">INFERMARK SCORE</span>
          <span className="display-medium">—</span>
          <span className="body-medium">
            {headline ?? "Run a benchmark to see how well this device runs local AI."}
          </span>
          {latest && !running && (
            <button className="btn text on-container" onClick={() => onShowResult(latest.path)}>
              See last result
              <Icon name="chevron_right" />
            </button>
          )}
        </div>

        {running ? (
          <div className="hero-actions">
            <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 8 }}>
              <span className="title-small">Benchmark running · {Math.round(overallProgress(run) * 100)}%</span>
              <Progress value={overallProgress(run)} />
            </div>
            <button className="btn filled" onClick={onOpenRun}>
              <Icon name="monitoring" />
              View
            </button>
          </div>
        ) : (
          <div className="hero-actions">
            <button className="btn filled large" onClick={() => onStart(profileKey)} disabled={!hw}>
              <Icon name="play_arrow" />
              Start benchmark
            </button>
            <label className="select-chip">
              <span className="visually-hidden">Profile</span>
              <select value={profileKey} onChange={(e) => onProfileChange(e.target.value)}>
                {(profiles.length ? profiles : [{ key: "quick", profile: { name: "Quick" } }]).map((p) => (
                  <option key={p.key} value={p.key}>
                    {p.profile.name}
                  </option>
                ))}
              </select>
              <Icon name="arrow_drop_down" small />
            </label>
          </div>
        )}
        {!running && (
          <button className="btn text on-container hero-link" onClick={onOpenRun}>
            See what will run
          </button>
        )}
      </section>

      {hw && (
        <section className="card" aria-label="Device summary">
          <div className="list">
            <SummaryRow icon="storage" title="Memory" live>
              {gb(hw.memory.available_bytes)} free of {gb(hw.memory.total_bytes, 0)}
            </SummaryRow>
            <SummaryRow icon={batteryIcon(hw)} title="Power" live>
              {powerLine(hw)}
            </SummaryRow>
            <SummaryRow icon="memory" title="Processor">
              {hw.cpu.physical_cores != null ? `${hw.cpu.physical_cores} cores · ` : ""}
              {hw.cpu.logical_cores} threads
            </SummaryRow>
            <SummaryRow icon="developer_board" title="Graphics">
              {hw.gpus.length === 0
                ? "None detected, models run on the processor"
                : hw.gpus.map((g) => `${cleanName(g.name)}${g.integrated ? " (shares memory)" : ""}`).join(", ")}
            </SummaryRow>
          </div>
          <div className="row spread">
            <button className="btn text" onClick={() => setShowAll(!showAll)} aria-expanded={showAll}>
              <Icon name={showAll ? "expand_less" : "expand_more"} />
              {showAll ? "Hide details" : "See all details"}
            </button>
            <button className="btn text" onClick={refresh} disabled={loading}>
              <Icon name="refresh" />
              {loading ? "Checking…" : "Refresh"}
            </button>
          </div>
        </section>
      )}

      {hw && showAll && <DeviceDetails hw={hw} />}

      {hw && (
        <p className="body-small muted row" style={{ gap: 6 }}>
          <Icon name="lock" small />
          Device ID {hw.fingerprint} is an anonymous hash of your hardware models. Nothing here leaves this device.
        </p>
      )}
    </div>
  );
}

function SummaryRow({ icon, title, live, children }: { icon: string; title: string; live?: boolean; children: ReactNode }) {
  return (
    <div className="list-item">
      <span className="lead">
        <Icon name={icon} />
      </span>
      <div className="content">
        <span className="body-large">{title}</span>
        <span className="body-medium muted">{children}</span>
      </div>
      {live && (
        <span className="live-badge label-medium" title="Updates every 2 seconds">
          <span className="live-dot" aria-hidden="true" />
          Live
        </span>
      )}
    </div>
  );
}

function powerLine(hw: HardwareInfo) {
  const source = hw.power_source === "battery" ? "On battery" : hw.power_source === "ac" ? "Plugged in" : "Power unknown";
  if (!hw.battery) return source;
  const state = hw.power_source === "battery" ? "" : hw.battery.charging ? ", charging" : "";
  return `${source} · ${hw.battery.charge_percent.toFixed(0)}%${state}`;
}

function DeviceDetails({ hw }: { hw: HardwareInfo }) {
  return (
    <div className="grid">
      <InfoCard icon="memory" title="Processor">
        <Kv k="Model" v={cleanName(hw.cpu.brand)} />
        <Kv k="Cores" v={hw.cpu.physical_cores != null ? `${hw.cpu.physical_cores}` : "Unknown"} />
        <Kv k="Threads" v={`${hw.cpu.logical_cores}`} />
        {hw.cpu.frequency_mhz && <Kv k="Clock" v={`${(hw.cpu.frequency_mhz / 1000).toFixed(2)} GHz`} />}
        <Kv k="Architecture" v={hw.os.arch} />
      </InfoCard>

      <InfoCard icon="storage" title="Memory">
        <Kv k="Installed" v={gb(hw.memory.total_bytes)} />
        <Kv k="Free now" v={gb(hw.memory.available_bytes)} />
        <MemoryBar total={hw.memory.total_bytes} free={hw.memory.available_bytes} />
      </InfoCard>

      {hw.gpus.map((g, i) => (
        <InfoCard key={i} icon="developer_board" title={hw.gpus.length > 1 ? `Graphics ${i + 1}` : "Graphics"}>
          <Kv k="Model" v={cleanName(g.name)} />
          <Kv k="Type" v={g.integrated ? "Integrated (shares RAM)" : "Dedicated"} />
          {!g.integrated && <Kv k="Video memory" v={gb(g.dedicated_vram_bytes)} />}
          {g.integrated && g.shared_memory_bytes != null && <Kv k="Can borrow" v={gb(g.shared_memory_bytes)} />}
          {g.driver_version && <Kv k="Driver" v={g.driver_version} />}
        </InfoCard>
      ))}

      <InfoCard icon="neurology" title="AI accelerator (NPU)">
        {hw.npus.length === 0 ? (
          <p className="body-medium muted">None detected. That's normal for most devices made before 2024.</p>
        ) : (
          hw.npus.map((n, i) => <Kv key={i} k={n.vendor || "NPU"} v={n.name} />)
        )}
      </InfoCard>

      <InfoCard icon={batteryIcon(hw)} title="Power">
        <Kv k="Source" v={hw.power_source === "battery" ? "Battery" : hw.power_source === "ac" ? "Plugged in" : "Unknown"} />
        {hw.battery ? (
          <>
            <Kv
              k="Charge"
              v={`${hw.battery.charge_percent.toFixed(0)}%${
                hw.power_source === "battery" ? " (discharging)" : hw.battery.charging ? " (charging)" : ""
              }`}
            />
            {hw.battery.full_capacity_wh && <Kv k="Capacity" v={`${hw.battery.full_capacity_wh} Wh`} />}
            {hw.battery.design_capacity_wh && hw.battery.full_capacity_wh && (
              <Kv k="Health" v={`${Math.round((hw.battery.full_capacity_wh / hw.battery.design_capacity_wh) * 100)}%`} />
            )}
          </>
        ) : (
          <Kv k="Battery" v="None" />
        )}
      </InfoCard>

      <InfoCard icon="info" title="System">
        <Kv k="OS" v={`${OS_LABEL[hw.os.family] ?? hw.os.name} ${hw.os.version}`} />
        <Kv k="Device ID" v={hw.fingerprint} />
      </InfoCard>
    </div>
  );
}

/** Battery icon that reflects charge level and charging state. */
function batteryIcon(hw: HardwareInfo) {
  const b = hw.battery;
  if (!b) return "power";
  if (b.charging || hw.power_source === "ac") return "battery_charging_full";
  const bars = Math.min(6, Math.max(0, Math.round((b.charge_percent / 100) * 6)));
  return bars === 6 ? "battery_full" : `battery_${bars}_bar`;
}

/** "11th Gen Intel(R) Core(TM) i5-11320H" -> "11th Gen Intel Core i5-11320H" */
function cleanName(name: string) {
  return name.replace(/\((R|TM)\)/gi, "").replace(/\s+/g, " ").trim();
}

/** Drop the clock-speed suffix for the headline: "... i5-11320H @ 3.20GHz" -> "... i5-11320H". */
function shortCpu(brand: string) {
  return cleanName(brand).replace(/\s*@.*$/, "").replace(/\s+(CPU|Processor)$/i, "");
}

function InfoCard({ icon, title, children }: { icon: string; title: string; children: ReactNode }) {
  return (
    <section className="card">
      <div className="card-head">
        <span className="avatar">
          <Icon name={icon} />
        </span>
        <h3 className="title-medium" style={{ flex: 1 }}>
          {title}
        </h3>
      </div>
      <dl className="kv body-medium">{children}</dl>
    </section>
  );
}

function Kv({ k, v }: { k: string; v: string }) {
  return (
    <>
      <dt>{k}</dt>
      <dd>{v}</dd>
    </>
  );
}

function MemoryBar({ total, free }: { total: number; free: number }) {
  const used = Math.max(0, Math.min(1, (total - free) / total));
  return (
    <div style={{ gridColumn: "1 / -1", display: "flex", flexDirection: "column", gap: 6 }}>
      <div className="progress" style={{ height: 8, borderRadius: 4 }}>
        <div style={{ width: `${used * 100}%`, borderRadius: 4 }} />
      </div>
      <span className="body-small muted">{Math.round(used * 100)}% in use by other apps</span>
    </div>
  );
}
