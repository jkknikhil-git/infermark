import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, gb, type HardwareInfo } from "../api";
import { Banner, Icon, PageHeader } from "../components/ui";

const LIVE_INTERVAL_MS = 2000;

const OS_LABEL: Record<string, string> = {
  windows: "Windows",
  macos: "macOS",
  linux: "Linux",
  android: "Android",
  ios: "iOS",
};

export default function DevicePage({ onRun }: { onRun: () => void }) {
  const [hw, setHw] = useState<HardwareInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

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

  return (
    <div className="page">
      <PageHeader
        title="This device"
        subtitle="What InferMark detected. Nothing here leaves your computer."
        actions={
          <button className="btn text" onClick={refresh} disabled={loading}>
            <Icon name="refresh" />
            {loading ? "Checking…" : "Refresh"}
          </button>
        }
      />

      {error && <Banner error icon="error">Couldn't read your hardware: {error}</Banner>}

      {hw && (
        <>
          <section className="card primary" aria-label="Summary">
            <div className="row spread" style={{ alignItems: "flex-start" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: 12, minWidth: 0, flex: "1 1 320px" }}>
                <span className="label-medium">YOUR DEVICE</span>
                <h2 className="title-large">{cleanName(hw.cpu.brand)}</h2>
                <div className="row" style={{ gap: 8 }}>
                  <span className="chip on-container">
                    <Icon name="desktop_windows" />
                    {OS_LABEL[hw.os.family] ?? hw.os.name} {hw.os.version}
                  </span>
                  <span className="chip on-container">
                    <Icon name="memory" />
                    {gb(hw.memory.total_bytes, 0)} RAM
                  </span>
                  {hw.gpus[0] && (
                    <span className="chip on-container">
                      <Icon name="developer_board" />
                      {cleanName(hw.gpus[0].name)}
                    </span>
                  )}
                  <span className="chip on-container">
                    <Icon name={hw.power_source === "battery" ? "battery_5_bar" : "power"} />
                    {hw.power_source === "battery" ? "On battery" : hw.power_source === "ac" ? "Plugged in" : "Power unknown"}
                  </span>
                </div>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 8, alignItems: "flex-start" }}>
                <span className="label-medium">INFERMARK SCORE</span>
                <span className="display-medium">—</span>
                <span className="body-medium">Run a benchmark to get your score.</span>
                <button className="btn filled" onClick={onRun} style={{ marginTop: 8 }}>
                  <Icon name="play_arrow" />
                  Start benchmark
                </button>
              </div>
            </div>
          </section>

          <div className="grid">
            <InfoCard icon="memory" title="Processor">
              <Kv k="Model" v={cleanName(hw.cpu.brand)} />
              <Kv k="Cores" v={hw.cpu.physical_cores != null ? `${hw.cpu.physical_cores}` : "Unknown"} />
              <Kv k="Threads" v={`${hw.cpu.logical_cores}`} />
              {hw.cpu.frequency_mhz && <Kv k="Clock" v={`${(hw.cpu.frequency_mhz / 1000).toFixed(2)} GHz`} />}
              <Kv k="Architecture" v={hw.os.arch} />
            </InfoCard>

            <InfoCard icon="storage" title="Memory" live>
              <Kv k="Installed" v={gb(hw.memory.total_bytes)} />
              <Kv k="Free now" v={gb(hw.memory.available_bytes)} />
              <MemoryBar total={hw.memory.total_bytes} free={hw.memory.available_bytes} />
            </InfoCard>

            {hw.gpus.length === 0 && (
              <InfoCard icon="developer_board" title="Graphics">
                <p className="body-medium muted">No GPU detected. Models will run on the processor.</p>
              </InfoCard>
            )}
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
                <p className="body-medium muted">None detected. That's normal for most PCs before 2024.</p>
              ) : (
                hw.npus.map((n, i) => <Kv key={i} k={n.vendor || "NPU"} v={n.name} />)
              )}
            </InfoCard>

            <InfoCard icon={batteryIcon(hw)} title="Power" live>
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
                <Kv k="Battery" v="None (desktop)" />
              )}
            </InfoCard>
          </div>

          <p className="body-small muted row" style={{ gap: 6 }}>
            <Icon name="lock" small />
            Device ID {hw.fingerprint} is an anonymous hash of your hardware models. It contains no personal information.
          </p>
        </>
      )}
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

function InfoCard({ icon, title, live, children }: { icon: string; title: string; live?: boolean; children: ReactNode }) {
  return (
    <section className="card">
      <div className="card-head">
        <span className="avatar">
          <Icon name={icon} />
        </span>
        <h3 className="title-medium" style={{ flex: 1 }}>
          {title}
        </h3>
        {live && (
          <span className="live-badge label-medium" title="Updates every 2 seconds">
            <span className="live-dot" aria-hidden="true" />
            Live
          </span>
        )}
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
