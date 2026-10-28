// Pre-run checks for the selected profile, refreshed while the user decides whether to start
// (closing an app or plugging in should clear a warning without a reload).
import { useEffect, useState } from "react";
import { api, type PreflightCheck } from "./api";

const REFRESH_MS = 15_000;

export function usePreflight(profileKey: string, active: boolean): PreflightCheck[] {
  const [checks, setChecks] = useState<PreflightCheck[]>([]);
  useEffect(() => {
    if (!active) return;
    let stale = false;
    const load = () =>
      api.preflightChecks(profileKey).then(
        (c) => !stale && setChecks(c),
        () => !stale && setChecks([]),
      );
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => {
      stale = true;
      window.clearInterval(timer);
    };
  }, [profileKey, active]);
  return checks;
}
