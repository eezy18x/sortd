// Types mirroring the Rust backend's serialized structures (camelCase).

export type Category =
  | "documents"
  | "images"
  | "videos"
  | "audio"
  | "archives"
  | "installers"
  | "development"
  | "code"
  | "spreadsheets"
  | "presentations"
  | "design"
  | "backups"
  | "other";

export type SortField = "name" | "size" | "modified" | "created" | "relevance";
export type SortOrder = "asc" | "desc";

export interface Page<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

export interface FileRecord {
  id: number;
  locationId: number;
  path: string;
  parent: string;
  name: string;
  extension: string | null;
  size: number;
  createdAt: number | null;
  modifiedAt: number | null;
  accessedAt: number | null;
  isHidden: boolean;
  isReadonly: boolean;
  isSymlink: boolean;
  isPlaceholder: boolean;
  mime: string | null;
  category: Category;
  categoryOverridden: boolean;
  categoryConfidence: number;
  fullHash: string | null;
}

export interface DirectoryRecord {
  path: string;
  name: string;
  parent: string | null;
  locationId: number;
  depth: number;
  ownSize: number;
  ownFiles: number;
  totalSize: number;
  totalFiles: number;
  totalDirs: number;
  complete: boolean;
  isHidden: boolean;
  modifiedAt: number | null;
}

export interface DirectoryListing {
  directory: DirectoryRecord;
  breadcrumbs: { name: string; path: string }[];
  subdirectories: DirectoryRecord[];
  files: Page<FileRecord>;
}

export interface Location {
  id: number;
  path: string;
  kind: "scan" | "destination";
  addedAt: number;
  lastScanId: number | null;
  lastScanAt: number | null;
  lastScanStatus: string | null;
  fileCount: number;
  totalSize: number;
  exists: boolean;
}

export interface CategoryStat {
  category: Category;
  files: number;
  size: number;
}

export interface ScanRecord {
  id: number;
  locationId: number;
  root: string;
  startedAt: number;
  finishedAt: number | null;
  status: "running" | "completed" | "cancelled" | "failed" | "interrupted";
  filesSeen: number;
  dirsSeen: number;
  bytesSeen: number;
  filesAdded: number;
  filesUpdated: number;
  filesRemoved: number;
  errors: number;
  skipped: number;
  message: string | null;
}

export interface Overview {
  totalFiles: number;
  totalSize: number;
  totalDirectories: number;
  locationCount: number;
  hashedFiles: number;
  duplicateGroups: number;
  duplicateWaste: number;
  pendingRecommendations: number;
  incompleteLocations: number;
  categories: CategoryStat[];
  largestFiles: FileRecord[];
  recentFiles: FileRecord[];
  recentScans: ScanRecord[];
}

export interface ScanProgress {
  scanId: number;
  locationId: number;
  root: string;
  phase: "walking" | "finalizing" | "done";
  status: string | null;
  filesSeen: number;
  dirsSeen: number;
  bytesSeen: number;
  errors: number;
  skipped: number;
  currentPath: string | null;
  elapsedMs: number;
}

export interface DuplicateProgress {
  phase: "grouping" | "partial" | "full" | "done";
  status: string | null;
  candidates: number;
  toHash: number;
  hashed: number;
  bytesToHash: number;
  bytesHashed: number;
  changed: number;
  errors: number;
  groups: number;
  reclaimable: number;
  elapsedMs: number;
}

export interface OperationProgress {
  batchId: number;
  phase: "running" | "undoing" | "done";
  done: number;
  total: number;
  current: string | null;
  status: string | null;
}

export interface JobStatus {
  scanRunning: boolean;
  duplicatesRunning: boolean;
  operationRunning: boolean;
  scan: ScanProgress | null;
  duplicates: DuplicateProgress | null;
  operation: OperationProgress | null;
}

export interface Classification {
  category: Category;
  confidence: number;
  reasons: string[];
  mimeHint: string | null;
}

export interface FileDetails {
  file: FileRecord;
  classification: Classification;
  signature: { mime: string; extension: string } | null;
  exists: boolean;
  unsafeToOpen: boolean;
  duplicates: FileRecord[];
}

export interface FileFilter {
  minSize?: number;
  maxSize?: number;
  categories?: Category[];
  extensions?: string[];
  locationId?: number;
  directory?: string;
  modifiedAfter?: number;
  modifiedBefore?: number;
  createdAfter?: number;
  createdBefore?: number;
  includeHidden?: boolean;
}

export interface SearchResponse {
  page: Page<FileRecord>;
  elapsedMs: number;
}

export interface Suggestion {
  text: string;
  source: "history" | "file";
}

export interface StorageAnalysis {
  indexedSize: number;
  indexedFiles: number;
  indexedDirectories: number;
  categories: CategoryStat[];
  largestDirectories: DirectoryRecord[];
  largestFiles: FileRecord[];
  ageBuckets: { label: string; files: number; size: number }[];
  extensions: { extension: string; files: number; size: number }[];
  duplicateGroups: number;
  duplicateWaste: number;
  volumes: {
    locationId: number;
    path: string;
    indexedSize: number;
    indexedFiles: number;
    complete: boolean;
    disk: { total: number; available: number } | null;
  }[];
  incompleteDirectories: number;
}

export interface DuplicateGroup {
  hash: string;
  size: number;
  count: number;
  reclaimable: number;
  files: FileRecord[];
}

export interface Recommendation {
  id: number;
  file: FileRecord;
  destination: string;
  reason: string;
  signals: string[];
  confidence: number;
  status: "pending" | "applied" | "dismissed";
  createdAt: number;
}

export type OperationRequest =
  | { op: "move"; source: string; destinationDir: string }
  | { op: "rename"; source: string; newName: string }
  | { op: "copy"; source: string; destinationDir: string }
  | { op: "createDirectory"; parent: string; name: string }
  | { op: "trash"; source: string };

export type ConflictPolicy = "skip" | "rename";

export interface PlanRequest {
  label: string;
  origin: "manual" | "organization" | "duplicates";
  operations: OperationRequest[];
  conflictPolicy: ConflictPolicy;
  allowCrossVolume: boolean;
}

export type OpKind = "move" | "rename" | "copy" | "mkdir" | "trash";

export interface OperationItem {
  id: number;
  seq: number;
  op: OpKind;
  source: string | null;
  destination: string | null;
  status: string;
  issue: string | null;
  error: string | null;
  crossVolume: boolean;
  isDirectory: boolean;
  size: number | null;
  verified: boolean;
}

export interface BatchSummary {
  id: number;
  label: string;
  origin: string;
  status: string;
  createdAt: number;
  executedAt: number | null;
  finishedAt: number | null;
  conflictPolicy: ConflictPolicy;
  allowCrossVolume: boolean;
  total: number;
  succeeded: number;
  failed: number;
  skipped: number;
  canUndo: boolean;
}

export interface Batch {
  summary: BatchSummary;
  items: OperationItem[];
  pending: number;
  conflicts: number;
  crossVolumeItems: number;
  totalBytes: number;
}

export interface DryRunItem {
  itemId: number;
  ok: boolean;
  message: string | null;
}

export interface ActivityEntry {
  id: number;
  tsMs: number;
  kind: string;
  summary: string;
  details: Record<string, unknown> | null;
  batchId: number | null;
}

export interface CategoryRule {
  extension: string;
  category: Category;
}

export interface Settings {
  theme: "system" | "dark" | "light";
  useDefaultExclusions: boolean;
  excludedPaths: string[];
  excludedNames: string[];
  includeHidden: boolean;
  crossFilesystems: boolean;
  hashConcurrency: number;
  scanOnStartup: boolean;
  detectDuplicatesAfterScan: boolean;
  duplicateMinSize: number;
  verifyContentType: boolean;
  categoryRules: CategoryRule[];
  confirmSingleOperations: boolean;
  defaultConflictPolicy: ConflictPolicy;
  recordSearchHistory: boolean;
  organizationRoot: string | null;
}

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
  databasePath: string;
  databaseSize: number;
  schemaVersion: number;
}

export interface IntegrityReport {
  ok: boolean;
  problems: string[];
  guidance: string | null;
}

export interface AppErrorPayload {
  kind: string;
  message: string;
}
