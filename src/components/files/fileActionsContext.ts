// The context lives in its own module so its identity is stable across hot
// reloads of FileActions.tsx.
import * as React from "react";
import type { FileRecord, PlanRequest } from "@/types";

export type PreviewRequest = Omit<PlanRequest, "conflictPolicy" | "allowCrossVolume">;

export interface FileActionsApi {
  details: (file: FileRecord) => void;
  open: (file: FileRecord) => void;
  reveal: (file: FileRecord) => void;
  rename: (file: FileRecord) => void;
  moveTo: (files: FileRecord[]) => void;
  copyTo: (files: FileRecord[]) => void;
  trash: (files: FileRecord[]) => void;
  /** Opens the preview dialog for an arbitrary plan. */
  preview: (request: PreviewRequest, onExecuted?: () => void) => void;
}

export const FileActionsContext = React.createContext<FileActionsApi | null>(null);

export function useFileActions(): FileActionsApi {
  const ctx = React.useContext(FileActionsContext);
  if (!ctx) throw new Error("useFileActions must be used inside FileActionsProvider");
  return ctx;
}
