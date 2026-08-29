import { useCallback, useEffect, useState } from "react";

/** Coordinates the initial and explicit application snapshot refreshes. */
export function useAppSnapshot(loadSnapshot: () => Promise<void>) {
  const [lastSnapshotAt, setLastSnapshotAt] = useState<Date>();
  const refresh = useCallback(async () => {
    await loadSnapshot();
    setLastSnapshotAt(new Date());
  }, [loadSnapshot]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { refresh, lastSnapshotAt };
}
