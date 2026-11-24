import { Link, useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  CircleAlert,
  Copy,
  FileBox,
  FolderPlus,
  FolderTree,
  HardDrive,
  LoaderCircle,
  RefreshCw,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import { api } from "@/services/api";
import type { FileRecord, ScanProgress } from "@/types";
import { categoryMeta } from "@/lib/categories";
import { formatBytes, formatCount, formatDuration, formatRelative } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardHeader, Progress, Skeleton } from "@/components/ui/primitives";
import {
  EmptyState,
  ErrorState,
  InlineNotice,
  Page,
  PageHeader,
  SizeBar,
  StatTile,
} from "@/components/common";
import { useFileActions } from "@/components/files/FileActions";
import { useJobs } from "@/hooks/useJobs";
import { useLocationActions } from "@/hooks/useLocationActions";

function ScanCard({ p }: { p: ScanProgress }) {
  return (
    <Card className="px-5 py-4">
      <div className="flex items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2.5">
          <LoaderCircle className="size-4 shrink-0 animate-spin text-accent" />
          <div className="min-w-0">
            <div className="text-[13.5px] font-medium">
              {p.phase === "finalizing" ? "Finalizing index" : "Scanning"} {p.root}
            </div>
            <div className="selectable truncate text-[12px] text-subtle">
              {p.currentPath ?? "Starting…"}
            </div>
          </div>
        </div>
        <Button size="sm" onClick={() => void api.cancelScan()}>
          Cancel
        </Button>
      </div>
      <Progress indeterminate className="mt-3" label="Scan progress" />
      <div className="tabular mt-3 flex flex-wrap gap-x-6 gap-y-1 text-[12.5px] text-muted">
        <span>
          <span className="font-medium text-fg">{formatCount(p.filesSeen)}</span> files
        </span>
        <span>
          <span className="font-medium text-fg">{formatCount(p.dirsSeen)}</span> folders
        </span>
        <span>
          <span className="font-medium text-fg">{formatBytes(p.bytesSeen)}</span> found
        </span>
        {p.errors > 0 && <span className="text-warning">{formatCount(p.errors)} unreadable</span>}
        <span>{formatDuration(p.elapsedMs)}</span>
      </div>
    </Card>
  );
}

function CompactFileList({
  files,
  meta,
}: {
  files: FileRecord[];
  meta: (f: FileRecord) => string;
}) {
  const a = useFileActions();
  if (files.length === 0)
    return <div className="px-5 pb-5 text-[13px] text-muted">Nothing indexed yet.</div>;
  return (
    <ul className="px-2 pb-2">
      {files.map((f) => {
        const Icon = categoryMeta(f.category).icon;
        return (
          <li key={f.id}>
            <button
              onClick={() => a.details(f)}
              className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface-2"
            >
              <Icon className="size-4 shrink-0 text-muted" strokeWidth={1.75} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px]">{f.name}</div>
                <div className="truncate text-[11.5px] text-subtle">{f.parent}</div>
              </div>
              <span className="tabular shrink-0 text-[12.5px] text-muted">{meta(f)}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export function OverviewPage() {
  const navigate = useNavigate();
  const jobs = useJobs();
  const { add, scan } = useLocationActions();
  const overview = useQuery({ queryKey: ["overview"], queryFn: api.overview });
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });

  if (overview.isError) {
    return (
      <Page>
        <ErrorState error={overview.error} onRetry={() => overview.refetch()} />
      </Page>
    );
  }

  const o = overview.data;
  const scanLocations = locations.data?.filter((l) => l.kind === "scan") ?? [];

  if (o && o.locationCount === 0 && !jobs.scanRunning) {
    return (
      <Page>
        <Card className="mt-8">
          <EmptyState
            icon={FolderPlus}
            title="Choose a folder to get started"
            description="SORTD indexes the folders you choose so you can search, analyze and organize them. Indexing only reads metadata — nothing is moved, changed or uploaded."
            action={
              <Button
                variant="primary"
                size="lg"
                onClick={() => add.mutate()}
                loading={add.isPending}
              >
                <FolderPlus /> Add a folder
              </Button>
            }
          />
          <div className="flex items-center justify-center gap-2 border-t border-border px-6 py-3 text-[12px] text-muted">
            <ShieldCheck className="size-4 text-accent" />
            Local-first: no account, no telemetry, no cloud. Your index stays on this computer.
          </div>
        </Card>
      </Page>
    );
  }

  const maxCat = Math.max(1, ...(o?.categories.map((c) => c.size) ?? [1]));

  return (
    <Page>
      <PageHeader
        title="Overview"
        description="A summary of everything SORTD has indexed. Sizes describe indexed files, not your whole disk."
        actions={
          <>
            <Button
              onClick={() => scan.mutate(undefined)}
              disabled={jobs.scanRunning || scanLocations.length === 0}
              loading={scan.isPending}
            >
              <RefreshCw /> Rescan all
            </Button>
            <Button variant="primary" onClick={() => add.mutate()} loading={add.isPending}>
              <FolderPlus /> Add folder
            </Button>
          </>
        }
      />

      {jobs.scanRunning && jobs.scan && <ScanCard p={jobs.scan} />}

      {o && o.incompleteLocations > 0 && (
        <InlineNotice tone="warning" icon={CircleAlert}>
          Some folders couldn’t be read completely (usually due to permissions). Their sizes are
          lower bounds.{" "}
          <Link to="/storage" className="underline underline-offset-2">
            See details
          </Link>
        </InlineNotice>
      )}

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-5">
        {o ? (
          <>
            <StatTile
              label="Indexed files"
              value={formatCount(o.totalFiles)}
              hint={`${formatCount(o.totalDirectories)} folders`}
              icon={FolderTree}
              onClick={() => navigate("/explorer")}
            />
            <StatTile
              label="Indexed size"
              value={formatBytes(o.totalSize)}
              hint={`${o.locationCount} location${o.locationCount === 1 ? "" : "s"}`}
              icon={HardDrive}
              onClick={() => navigate("/storage")}
            />
            <StatTile
              label="Reclaimable"
              value={formatBytes(o.duplicateWaste)}
              hint={`${formatCount(o.duplicateGroups)} duplicate groups`}
              icon={Copy}
              onClick={() => navigate("/duplicates")}
            />
            <StatTile
              label="Suggestions"
              value={formatCount(o.pendingRecommendations)}
              hint="Organization ideas"
              icon={Sparkles}
              onClick={() => navigate("/organize")}
            />
            <StatTile
              label="Largest file"
              value={o.largestFiles[0] ? formatBytes(o.largestFiles[0].size) : "—"}
              hint={o.largestFiles[0]?.name ?? "No files yet"}
              icon={FileBox}
              onClick={() => navigate("/large")}
            />
          </>
        ) : (
          Array.from({ length: 5 }, (_, i) => <Skeleton key={i} className="h-[92px] rounded-xl" />)
        )}
      </div>

      <div className="grid gap-4 lg:grid-cols-[1.25fr_1fr]">
        <Card>
          <CardHeader
            title="Storage by category"
            description="Indexed size per category"
            actions={
              <Button variant="ghost" size="sm" onClick={() => navigate("/categories")}>
                View all
              </Button>
            }
          />
          <ul className="flex flex-col gap-2.5 px-5 pb-5">
            {o?.categories.length === 0 && (
              <li className="text-[13px] text-muted">No files indexed yet.</li>
            )}
            {o?.categories.map((c) => {
              const meta = categoryMeta(c.category);
              return (
                <li key={c.category}>
                  <button
                    className="group w-full text-left"
                    onClick={() => navigate(`/categories?c=${c.category}`)}
                  >
                    <div className="mb-1 flex items-center justify-between text-[12.5px]">
                      <span className="flex items-center gap-2">
                        <meta.icon className="size-3.5 text-muted" strokeWidth={1.75} />
                        <span className="group-hover:text-fg">{meta.label}</span>
                        <span className="tabular text-subtle">{formatCount(c.files)}</span>
                      </span>
                      <span className="tabular text-muted">{formatBytes(c.size)}</span>
                    </div>
                    <SizeBar fraction={c.size / maxCat} />
                  </button>
                </li>
              );
            })}
          </ul>
        </Card>

        <Card>
          <CardHeader
            title="Locations"
            description="Folders SORTD is authorized to index"
            actions={
              <Button variant="ghost" size="sm" onClick={() => navigate("/settings#locations")}>
                Manage
              </Button>
            }
          />
          <ul className="px-2 pb-2">
            {scanLocations.map((l) => (
              <li key={l.id} className="flex items-center gap-3 rounded-lg px-3 py-2">
                <HardDrive className="size-4 shrink-0 text-muted" strokeWidth={1.75} />
                <div className="min-w-0 flex-1">
                  <div className="selectable truncate text-[13px]" title={l.path}>
                    {l.path}
                  </div>
                  <div className="text-[11.5px] text-subtle">
                    {l.lastScanAt ? (
                      <>
                        {formatCount(l.fileCount)} files · {formatBytes(l.totalSize)} · scanned{" "}
                        {formatRelative(l.lastScanAt)}
                      </>
                    ) : (
                      "Not scanned yet"
                    )}
                  </div>
                </div>
                {!l.exists ? (
                  <Badge tone="danger">Missing</Badge>
                ) : l.lastScanStatus && l.lastScanStatus !== "completed" ? (
                  <Badge tone="warning">{l.lastScanStatus}</Badge>
                ) : null}
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Rescan ${l.path}`}
                  disabled={jobs.scanRunning || !l.exists}
                  onClick={() => scan.mutate([l.id])}
                >
                  <RefreshCw />
                </Button>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader
            title="Largest files"
            actions={
              <Button variant="ghost" size="sm" onClick={() => navigate("/large")}>
                View all
              </Button>
            }
          />
          {o ? (
            <CompactFileList files={o.largestFiles} meta={(f) => formatBytes(f.size)} />
          ) : (
            <Skeleton className="mx-5 mb-5 h-40" />
          )}
        </Card>
        <Card>
          <CardHeader
            title="Recently modified"
            actions={
              <Button variant="ghost" size="sm" onClick={() => navigate("/recent")}>
                View all
              </Button>
            }
          />
          {o ? (
            <CompactFileList files={o.recentFiles} meta={(f) => formatRelative(f.modifiedAt)} />
          ) : (
            <Skeleton className="mx-5 mb-5 h-40" />
          )}
        </Card>
      </div>

      {o && o.recentScans.length > 0 && (
        <Card>
          <CardHeader
            title="Recent scans"
            actions={
              <Button variant="ghost" size="sm" onClick={() => navigate("/history")}>
                Activity
              </Button>
            }
          />
          <div className="overflow-x-auto px-5 pb-4">
            <table className="w-full text-[12.5px]">
              <thead className="text-left text-[11.5px] text-muted">
                <tr className="border-b border-border">
                  <th className="py-2 font-medium">Location</th>
                  <th className="py-2 font-medium">Status</th>
                  <th className="py-2 text-right font-medium">Files</th>
                  <th className="py-2 text-right font-medium">Added</th>
                  <th className="py-2 text-right font-medium">Updated</th>
                  <th className="py-2 text-right font-medium">Removed</th>
                  <th className="py-2 text-right font-medium">Unreadable</th>
                  <th className="py-2 text-right font-medium">When</th>
                </tr>
              </thead>
              <tbody className="tabular">
                {o.recentScans.map((s) => (
                  <tr key={s.id} className="border-b border-border/60 last:border-0">
                    <td className="selectable max-w-[280px] truncate py-2" title={s.root}>
                      {s.root}
                    </td>
                    <td className="py-2">
                      <Badge
                        tone={
                          s.status === "completed"
                            ? "success"
                            : s.status === "running"
                              ? "accent"
                              : "warning"
                        }
                      >
                        {s.status}
                      </Badge>
                    </td>
                    <td className="py-2 text-right">{formatCount(s.filesSeen)}</td>
                    <td className="py-2 text-right">{formatCount(s.filesAdded)}</td>
                    <td className="py-2 text-right">{formatCount(s.filesUpdated)}</td>
                    <td className="py-2 text-right">{formatCount(s.filesRemoved)}</td>
                    <td className="py-2 text-right">
                      {s.errors > 0 ? (
                        <span className="text-warning">{formatCount(s.errors)}</span>
                      ) : (
                        "0"
                      )}
                    </td>
                    <td className="py-2 text-right text-muted">
                      {formatRelative(s.finishedAt ?? s.startedAt)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}
    </Page>
  );
}
