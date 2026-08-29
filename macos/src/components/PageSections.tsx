import type { ReactNode } from "react";

/**
 * Stable page seams used by App's shell.  Keeping the page roots as named
 * components lets the shell own navigation while each flow can be extracted
 * or tested independently without changing layout classes.
 */
export function InstallFlow({ children }: { children: ReactNode }) {
  return <div className="page install-page">{children}</div>;
}

export function InventoryPage({ children }: { children: ReactNode }) {
  return <div className="page manage-page">{children}</div>;
}

export function OperationsPage({ children }: { children: ReactNode }) {
  return <div className="page operations-page">{children}</div>;
}

export function DiagnosticsPage({ children }: { children: ReactNode }) {
  return <div className="page diagnostics-page">{children}</div>;
}
