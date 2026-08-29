import { useEffect, useState } from "react";
import { api } from "../api";
import type { EnvironmentScan } from "../types";

/**
 * Polls known local Skill roots while the inventory page is visible.  This is
 * intentionally local-only; network update checks remain explicit actions.
 */
export function useInventoryEvents(
  enabled: boolean,
  onEnvironment: (environment: EnvironmentScan) => void,
) {
  const [lastInventoryScanAt, setLastInventoryScanAt] = useState<Date>();

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const scan = () => {
      void api
        .scanEnvironment()
        .then((nextEnvironment) => {
          if (!active) return;
          onEnvironment(nextEnvironment);
          setLastInventoryScanAt(new Date());
        })
        .catch(() => {
          // Keep the last good inventory visible. Manual refresh surfaces a
          // detailed error when the issue persists.
        });
    };
    const timer = window.setInterval(scan, 5000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [enabled, onEnvironment]);

  return { lastInventoryScanAt };
}
