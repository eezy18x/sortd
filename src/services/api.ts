// Typed wrappers around the backend's IPC commands. This is the only module
// that talks to Tauri; components use these functions (usually through
// TanStack Query) and never call `invoke` directly.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ActivityEntry,
  AppErrorPayload,
  AppInfo,
  Batch,
  BatchSummary,
  CategoryStat,
  DirectoryListing,
  DryRunItem,
  DuplicateGroup,
  DuplicateProgress,
  FileDetails,
  FileFilter,
  FileRecord,
  IntegrityReport,
  JobStatus,
  Location,
  OperationProgress,
  Overview,
  Page,
  PlanRequest,
  Recommendation,
  ScanProgress,
  ScanRecord,
  SearchResponse,
  Settings,
  SortField,
  SortOrder,
  StorageAnalysis,
  Suggestion,
} from "@/types";

export class ApiError extends Error {
  readonly kind: string;
  constructor(kind: string, message: string) {
    super(message);
    this.kind = kind;
    this.name = "ApiError";
  }
}

/** True when running inside the Tauri webview (false in a plain browser). */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new ApiError(
      "unavailable",
      "The SORTD backend is not available. Run the app with `pnpm tauri dev`.",
    );
  }
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    if (e && typeof e === "object" && "kind" in e && "message" in e) {
      const p = e as AppErrorPayload;
      throw new ApiError(p.kind, p.message);
    }
    throw new ApiError("internal", typeof e === "string" ? e : String(e));
  }
}

export const errorMessage = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : "Something went wrong";

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  overview: () => call<Overview>("get_overview"),
  jobStatus: () => call<JobStatus>("get_job_status"),

  locations: () => call<Location[]>("list_locations"),
  addLocation: (kind: "scan" | "destination") => call<Location | null>("add_location", { kind }),
  removeLocation: (id: number) => call<void>("remove_location", { id }),
  startScan: (locationIds?: number[]) =>
    call<void>("start_scan", { locationIds: locationIds ?? null }),
  cancelScan: () => call<boolean>("cancel_scan"),
  scans: (limit = 50) => call<ScanRecord[]>("list_scans", { limit }),

  listDirectory: (path: string, sort?: SortField, order?: SortOrder, limit = 500, offset = 0) =>
    call<DirectoryListing>("list_directory", { path, sort, order, limit, offset }),
  fileDetails: (id: number) => call<FileDetails>("get_file_details", { id }),
  setCategoryOverride: (id: number, category: string | null) =>
    call<FileRecord>("set_category_override", { id, category }),
  queryFiles: (filter: FileFilter, sort?: SortField, order?: SortOrder, limit = 200, offset = 0) =>
    call<Page<FileRecord>>("query_files", { filter, sort, order, limit, offset }),
  categoryStats: (locationId?: number) =>
    call<CategoryStat[]>("category_stats", { locationId: locationId ?? null }),

  search: (request: {
    query: string;
    filter?: FileFilter;
    sort?: SortField;
    order?: SortOrder;
    limit?: number;
    offset?: number;
  }) =>
    call<SearchResponse>("search_files", {
      request: { filter: {}, limit: 200, offset: 0, ...request },
    }),
  suggestions: (prefix: string) => call<Suggestion[]>("search_suggestions", { prefix }),
  searchHistory: () => call<string[]>("search_history"),
  recordSearch: (query: string) => call<void>("record_search", { query }),
  clearSearchHistory: () => call<void>("clear_search_history"),

  storage: (locationId?: number) =>
    call<StorageAnalysis>("storage_analysis", { locationId: locationId ?? null }),

  startDuplicateScan: () => call<void>("start_duplicate_scan"),
  cancelDuplicateScan: () => call<boolean>("cancel_duplicate_scan"),
  duplicateGroups: (sort = "waste", limit = 50, offset = 0) =>
    call<Page<DuplicateGroup>>("list_duplicate_groups", { sort, limit, offset }),

  generateRecommendations: () =>
    call<{ examined: number; generated: number; removed: number }>("generate_recommendations"),
  recommendations: (status = "pending", minConfidence = 0, limit = 200, offset = 0) =>
    call<Page<Recommendation>>("list_recommendations", { status, minConfidence, limit, offset }),
  setRecommendationStatus: (ids: number[], status: "pending" | "dismissed") =>
    call<number>("set_recommendation_status", { ids, status }),

  planOperations: (request: PlanRequest) => call<Batch>("plan_operations", { request }),
  dryRun: (batchId: number) => call<DryRunItem[]>("dry_run_operations", { batchId }),
  executeOperations: (batchId: number) => call<Batch>("execute_operations", { batchId }),
  cancelOperations: () => call<boolean>("cancel_operations"),
  undoBatch: (batchId: number) => call<Batch>("undo_batch", { batchId }),
  discardPlan: (batchId: number) => call<void>("discard_plan", { batchId }),
  batches: (limit = 50, offset = 0) =>
    call<{ items: BatchSummary[]; total: number }>("list_batches", { limit, offset }),
  batch: (batchId: number) => call<Batch>("get_batch", { batchId }),

  activity: (kinds: string[] = [], limit = 100, offset = 0) =>
    call<{ items: ActivityEntry[]; total: number }>("list_activity", { kinds, limit, offset }),

  settings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) =>
    call<Settings>("update_settings", { update: { settings } }),
  pickOrganizationRoot: () => call<Settings>("pick_organization_root"),
  clearOrganizationRoot: () => call<Settings>("clear_organization_root"),

  openFile: (id: number) => call<void>("open_file", { id }),
  revealFile: (id: number) => call<void>("reveal_file", { id }),
  revealDirectory: (path: string) => call<void>("reveal_directory", { path }),

  checkDatabase: () => call<IntegrityReport>("check_database"),
  optimizeDatabase: () => call<void>("optimize_database"),
  resetIndex: () => call<void>("reset_index"),
};

export const events = {
  scan: "sortd://scan-progress",
  duplicates: "sortd://duplicates-progress",
  operation: "sortd://operation-progress",
  indexChanged: "sortd://index-changed",
} as const;

type EventPayloads = {
  [events.scan]: ScanProgress;
  [events.duplicates]: DuplicateProgress;
  [events.operation]: OperationProgress;
  [events.indexChanged]: null;
};

/** Subscribes to a backend event. Resolves to a no-op outside Tauri. */
export async function subscribe<K extends keyof EventPayloads>(
  event: K,
  handler: (payload: EventPayloads[K]) => void,
): Promise<UnlistenFn> {
  if (!isTauri()) return () => {};
  return listen<EventPayloads[K]>(event, (e) => handler(e.payload));
}
