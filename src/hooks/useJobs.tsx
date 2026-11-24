import * as React from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api, events, isTauri, subscribe } from "@/services/api";
import type { DuplicateProgress, JobStatus, OperationProgress, ScanProgress } from "@/types";

interface JobsState {
  scan: ScanProgress | null;
  scanRunning: boolean;
  duplicates: DuplicateProgress | null;
  duplicatesRunning: boolean;
  operation: OperationProgress | null;
  operationRunning: boolean;
}

const initial: JobsState = {
  scan: null,
  scanRunning: false,
  duplicates: null,
  duplicatesRunning: false,
  operation: null,
  operationRunning: false,
};

const JobsContext = React.createContext<JobsState>(initial);

/**
 * Tracks background jobs via backend events and keeps cached queries fresh.
 * Index-change events are coalesced so a burst of changes triggers a single
 * refetch instead of re-rendering the UI for every filesystem event.
 */
export function JobsProvider({ children }: { children: React.ReactNode }) {
  const qc = useQueryClient();
  const [state, setState] = React.useState<JobsState>(initial);

  React.useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    let timer: number | undefined;
    const invalidate = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => void qc.invalidateQueries(), 250);
    };

    api
      .jobStatus()
      .then((s: JobStatus) => {
        if (!disposed) setState((prev) => ({ ...prev, ...s }));
      })
      .catch(() => {});

    void (async () => {
      const subs = await Promise.all([
        subscribe(events.scan, (p) =>
          setState((s) => ({ ...s, scan: p, scanRunning: p.phase !== "done" })),
        ),
        subscribe(events.duplicates, (p) =>
          setState((s) => ({ ...s, duplicates: p, duplicatesRunning: p.phase !== "done" })),
        ),
        subscribe(events.operation, (p) =>
          setState((s) => ({ ...s, operation: p, operationRunning: p.phase !== "done" })),
        ),
        subscribe(events.indexChanged, () => {
          invalidate();
          // Authoritative running flags (a multi-location scan emits "done" per location).
          api
            .jobStatus()
            .then((s) => setState((prev) => ({ ...prev, ...s })))
            .catch(() => {});
        }),
      ]);
      if (disposed) subs.forEach((u) => u());
      else unlisteners.push(...subs);
    })();

    return () => {
      disposed = true;
      window.clearTimeout(timer);
      unlisteners.forEach((u) => u());
    };
  }, [qc]);

  return <JobsContext.Provider value={state}>{children}</JobsContext.Provider>;
}

export function useJobs(): JobsState {
  return React.useContext(JobsContext);
}
