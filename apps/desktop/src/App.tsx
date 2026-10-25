import { Fragment, useEffect, useState } from "react";
import { Icon } from "./components/ui";
import { isPreview } from "./api";
import { applyTheme, loadPreferences, savePreferences, type Preferences } from "./theme";
import { useRunState } from "./run-state";
import DevicePage from "./pages/DevicePage";
import RunPage from "./pages/RunPage";
import ResultsPage from "./pages/ResultsPage";
import PacksPage from "./pages/PacksPage";
import SettingsPage from "./pages/SettingsPage";
import { ComparePage, ModelsPage } from "./pages/ComingSoon";

export type PageId = "device" | "run" | "results" | "compare" | "models" | "packs" | "settings";

const NAV: { id: PageId; label: string; icon: string }[] = [
  { id: "device", label: "Device", icon: "devices" },
  { id: "run", label: "Run", icon: "speed" },
  { id: "results", label: "Results", icon: "insights" },
  { id: "compare", label: "Compare", icon: "compare_arrows" },
  { id: "models", label: "Models", icon: "deployed_code" },
  { id: "packs", label: "Tests", icon: "fact_check" },
  { id: "settings", label: "Settings", icon: "settings" },
];

export default function App() {
  const [page, setPage] = useState<PageId>("device");
  const [prefs, setPrefs] = useState<Preferences>(loadPreferences);
  const [run, dispatchRun] = useRunState();
  const [openResult, setOpenResult] = useState<string | null>(null);

  useEffect(() => {
    applyTheme(prefs.accent, prefs.mode);
    savePreferences(prefs);
    if (prefs.mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme(prefs.accent, prefs.mode);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [prefs]);

  const go = (id: PageId) => {
    setPage(id);
    if (id !== "results") setOpenResult(null);
  };

  const showResult = (path: string) => {
    setOpenResult(path);
    setPage("results");
  };

  return (
    <div className="app">
      <nav className="rail" aria-label="Main">
        <div className="rail-brand" title="InferMark">
          <Icon name="speed" filled />
        </div>
        {NAV.map((item, i) => (
          <Fragment key={item.id}>
            {i === NAV.length - 1 && <div className="rail-spacer" />}
            <button
              className="rail-item"
              aria-current={page === item.id ? "page" : undefined}
              onClick={() => go(item.id)}
            >
              <span className="rail-indicator">
                {item.id === "run" && run.phase === "running" ? (
                  <span className="spin">
                    <Icon name="progress_activity" />
                  </span>
                ) : (
                  <Icon name={item.icon} filled={page === item.id} />
                )}
              </span>
              <span className="label-medium">{item.label}</span>
            </button>
          </Fragment>
        ))}
      </nav>

      <main className="main">
        {isPreview && (
          <div className="page" style={{ paddingBottom: 0 }}>
            <div className="banner">
              <Icon name="visibility" small />
              <span className="body-medium">Preview mode: showing sample data. Run the desktop app to test your own device.</span>
            </div>
          </div>
        )}
        {page === "device" && <DevicePage onRun={() => go("run")} />}
        {page === "run" && <RunPage run={run} dispatch={dispatchRun} onShowResult={showResult} />}
        {page === "results" && <ResultsPage openPath={openResult} onOpen={setOpenResult} onRun={() => go("run")} />}
        {page === "compare" && <ComparePage />}
        {page === "models" && <ModelsPage />}
        {page === "packs" && <PacksPage />}
        {page === "settings" && <SettingsPage prefs={prefs} onChange={setPrefs} />}
      </main>
    </div>
  );
}
