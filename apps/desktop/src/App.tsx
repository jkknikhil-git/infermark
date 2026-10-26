import { useEffect, useState } from "react";
import { Icon } from "./components/ui";
import { api, isPreview } from "./api";
import { applyTheme, loadPreferences, savePreferences, type Preferences } from "./theme";
import { useRunState } from "./run-state";
import HomePage from "./pages/DevicePage";
import RunPage from "./pages/RunPage";
import ResultsPage from "./pages/ResultsPage";
import PacksPage from "./pages/PacksPage";
import SettingsPage from "./pages/SettingsPage";
import { ComparePage } from "./pages/ComingSoon";
import ModelsPage from "./pages/ModelsPage";

/** Every screen. Only the four top-level ones appear in the navigation bar. */
export type PageId = "home" | "run" | "results" | "compare" | "models" | "settings" | "tests";
type NavId = "home" | "results" | "models" | "settings";

// Material 3 navigation bars hold 3-5 destinations; everything else is reached from these.
const NAV: { id: NavId; label: string; icon: string }[] = [
  { id: "home", label: "Home", icon: "home" },
  { id: "results", label: "Results", icon: "insights" },
  { id: "models", label: "Models", icon: "deployed_code" },
  { id: "settings", label: "Settings", icon: "settings" },
];

const PARENT: Record<PageId, NavId> = {
  home: "home",
  run: "home",
  results: "results",
  compare: "results",
  models: "models",
  settings: "settings",
  tests: "settings",
};

export default function App() {
  const [page, setPage] = useState<PageId>("home");
  const [prefs, setPrefs] = useState<Preferences>(loadPreferences);
  const [run, dispatchRun] = useRunState();
  const [openResult, setOpenResult] = useState<string | null>(null);
  const [profileKey, setProfileKey] = useState("quick");
  const [backgrounded, setBackgrounded] = useState(false);

  useEffect(() => {
    applyTheme(prefs.accent, prefs.mode);
    savePreferences(prefs);
    if (prefs.mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme(prefs.accent, prefs.mode);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [prefs]);

  // While a benchmark runs: keep the screen on, and note if the app goes to the background
  // (phones and laptops slow down background apps, which would make results look worse).
  const running = run.phase === "running";
  useEffect(() => {
    if (!running) return;
    let lock: WakeLockSentinel | null = null;
    const acquire = () => {
      if (document.visibilityState !== "visible" || !("wakeLock" in navigator)) return;
      navigator.wakeLock.request("screen").then(
        (l) => (lock = l),
        () => {
          /* not supported here; the run still works */
        },
      );
    };
    const onVisibility = () => {
      if (document.hidden) setBackgrounded(true);
      else acquire();
    };
    acquire();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      lock?.release().catch(() => {});
    };
  }, [running]);

  const go = (id: PageId) => {
    setPage(id);
    if (id !== "results") setOpenResult(null);
  };

  const showResult = (path: string) => {
    setOpenResult(path);
    setPage("results");
  };

  const startRun = async (key: string) => {
    setProfileKey(key);
    setBackgrounded(false);
    dispatchRun({ type: "start", profileKey: key });
    setPage("run");
    try {
      await api.startRun(key);
    } catch (e) {
      dispatchRun({ type: "error", message: String(e) });
    }
  };

  const current = PARENT[page];

  return (
    <div className={page === "run" ? "app flow" : "app"}>
      <nav className="rail" aria-label="Main">
        <div className="rail-brand" title="InferMark">
          <Icon name="speed" filled />
        </div>
        {NAV.map((item) => (
          <button
            key={item.id}
            className="rail-item"
            aria-current={current === item.id ? "page" : undefined}
            onClick={() => go(item.id)}
          >
            <span className="rail-indicator">
              {item.id === "home" && running ? (
                <span className="spin">
                  <Icon name="progress_activity" />
                </span>
              ) : (
                <Icon name={item.icon} filled={current === item.id} />
              )}
            </span>
            <span className="label-medium">{item.label}</span>
          </button>
        ))}
      </nav>

      <main className="main">
        {isPreview && (
          <div className="page" style={{ paddingBottom: 0 }}>
            <div className="banner">
              <Icon name="visibility" small />
              <span className="body-medium">Preview mode: showing sample data. Install InferMark to test your own device.</span>
            </div>
          </div>
        )}
        {page === "home" && (
          <HomePage
            run={run}
            profileKey={profileKey}
            onProfileChange={setProfileKey}
            onStart={startRun}
            onOpenRun={() => go("run")}
            onShowResult={showResult}
          />
        )}
        {page === "run" && (
          <RunPage
            run={run}
            dispatch={dispatchRun}
            profileKey={profileKey}
            onProfileChange={setProfileKey}
            onStart={startRun}
            backgrounded={backgrounded}
            onShowResult={showResult}
            onClose={() => go("home")}
          />
        )}
        {page === "results" && (
          <ResultsPage
            openPath={openResult}
            onOpen={setOpenResult}
            onRun={() => go("home")}
            onCompare={() => go("compare")}
          />
        )}
        {page === "compare" && <ComparePage onBack={() => go("results")} />}
        {page === "models" && <ModelsPage onOpenSettings={() => go("settings")} />}
        {page === "tests" && <PacksPage onBack={() => go("settings")} />}
        {page === "settings" && <SettingsPage prefs={prefs} onChange={setPrefs} onOpenTests={() => go("tests")} />}
      </main>
    </div>
  );
}
