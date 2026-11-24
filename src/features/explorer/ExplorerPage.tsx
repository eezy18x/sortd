import * as React from "react";
import { useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  ChevronRight,
  CircleAlert,
  Copy,
  Ellipsis,
  Folder,
  FolderInput,
  FolderOpen,
  FolderPlus,
  FolderTree,
  HardDrive,
  Pencil,
  RefreshCw,
  Trash2,
  X,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type { DirectoryRecord, FileRecord } from "@/types";
import { formatBytes, formatCount, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  Menu,
  MenuContent,
  MenuItem,
  MenuSeparator,
  MenuTrigger,
} from "@/components/ui/overlays";
import { Badge, Card, Input, Tooltip } from "@/components/ui/primitives";
import { EmptyState, ErrorState, LoadingState, SizeBar } from "@/components/common";
import { FileTable, useInfiniteFiles, type SortState } from "@/components/files/FileTable";
import { useFileActions } from "@/components/files/FileActions";
import { DirectoryPicker } from "@/components/operations/DirectoryPicker";
import { useToast } from "@/components/toast";
import { invalidNameReason } from "@/lib/validation";
import { useJobs } from "@/hooks/useJobs";
import { useLocationActions } from "@/hooks/useLocationActions";

function NameDialog({
  title,
  initial,
  confirmLabel,
  onSubmit,
  onClose,
}: {
  title: string;
  initial: string;
  confirmLabel: string;
  onSubmit: (name: string) => void;
  onClose: () => void;
}) {
  const [name, setName] = React.useState(initial);
  const trimmed = name.trim();
  const reason = invalidNameReason(trimmed);
  const invalid = reason !== null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={title}
        footer={
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={invalid || trimmed === initial}
              onClick={() => onSubmit(trimmed)}
            >
              {confirmLabel}
            </Button>
          </>
        }
      >
        <form
          className="p-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (!invalid && trimmed !== initial) onSubmit(trimmed);
          }}
        >
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            aria-label="Name"
            aria-invalid={invalid}
          />
          {invalid && trimmed && <p className="mt-2 text-[12px] text-danger">{reason}</p>}
        </form>
      </DialogContent>
    </Dialog>
  );
}

function FolderRow({ d, max, onOpen }: { d: DirectoryRecord; max: number; onOpen: () => void }) {
  const actions = useFileActions();
  const toast = useToast();
  const [dialog, setDialog] = React.useState<"rename" | "move" | null>(null);
  return (
    <li className="group grid grid-cols-[minmax(0,1fr)_160px_96px_32px] items-center gap-3 border-b border-border/60 px-4 last:border-0 hover:bg-surface-2">
      <button className="flex min-w-0 items-center gap-2.5 py-2 text-left" onClick={onOpen}>
        <Folder className="size-4 shrink-0 text-accent" strokeWidth={1.75} />
        <span className="truncate text-[13px]">{d.name}</span>
        {!d.complete && (
          <Tooltip content="Part of this folder couldn’t be read; its size is a lower bound.">
            <CircleAlert className="size-3.5 shrink-0 text-warning" aria-label="Incomplete" />
          </Tooltip>
        )}
        <span className="tabular shrink-0 text-[11.5px] text-subtle">
          {formatCount(d.totalFiles)} files
        </span>
      </button>
      <SizeBar fraction={max > 0 ? d.totalSize / max : 0} />
      <span className="tabular text-right text-[12.5px]">
        {!d.complete && "≥ "}
        {formatBytes(d.totalSize)}
      </span>
      <Menu>
        <MenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={`Actions for folder ${d.name}`}>
            <Ellipsis />
          </Button>
        </MenuTrigger>
        <MenuContent>
          <MenuItem icon={<FolderTree />} onSelect={onOpen}>
            Open in SORTD
          </MenuItem>
          <MenuItem
            icon={<FolderOpen />}
            onSelect={() =>
              api
                .revealDirectory(d.path)
                .catch((e) => toast.error("Couldn’t show the folder", errorMessage(e)))
            }
          >
            Show in file manager
          </MenuItem>
          <MenuSeparator />
          <MenuItem icon={<Pencil />} onSelect={() => setDialog("rename")}>
            Rename…
          </MenuItem>
          <MenuItem icon={<FolderInput />} onSelect={() => setDialog("move")}>
            Move to…
          </MenuItem>
          <MenuSeparator />
          <MenuItem
            icon={<Trash2 />}
            danger
            onSelect={() =>
              actions.preview({
                label: `Move folder “${d.name}” to trash`,
                origin: "manual",
                operations: [{ op: "trash", source: d.path }],
              })
            }
          >
            Move to trash…
          </MenuItem>
        </MenuContent>
      </Menu>
      {dialog === "rename" && (
        <NameDialog
          title="Rename folder"
          initial={d.name}
          confirmLabel="Review rename"
          onClose={() => setDialog(null)}
          onSubmit={(newName) => {
            setDialog(null);
            actions.preview({
              label: `Rename folder “${d.name}”`,
              origin: "manual",
              operations: [{ op: "rename", source: d.path, newName }],
            });
          }}
        />
      )}
      {dialog === "move" && (
        <DirectoryPicker
          title={`Move folder “${d.name}”`}
          confirmLabel="Review move"
          onClose={() => setDialog(null)}
          onPick={(dir) => {
            setDialog(null);
            actions.preview({
              label: `Move folder “${d.name}”`,
              origin: "manual",
              operations: [{ op: "move", source: d.path, destinationDir: dir }],
            });
          }}
        />
      )}
    </li>
  );
}

export function ExplorerPage() {
  const [params, setParams] = useSearchParams();
  const actions = useFileActions();
  const jobs = useJobs();
  const { add, scan } = useLocationActions();
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });
  const scanLocations = React.useMemo(
    () => locations.data?.filter((l) => l.kind === "scan") ?? [],
    [locations.data],
  );
  const path = params.get("path") ?? scanLocations.find((l) => l.lastScanId)?.path ?? null;
  const [sort, setSort] = React.useState<SortState>({ field: "name", order: "asc" });
  const [selection, setSelection] = React.useState<Set<number>>(new Set());
  const [newFolder, setNewFolder] = React.useState(false);

  const go = (p: string) => {
    setSelection(new Set());
    setParams({ path: p });
  };

  const listing = useQuery({
    queryKey: ["directory", path, sort.field, sort.order],
    queryFn: () => api.listDirectory(path!, sort.field, sort.order, 1, 0),
    enabled: !!path,
  });
  const files = useInfiniteFiles(
    ["directory-files", path, sort.field, sort.order],
    (offset, limit) =>
      api.listDirectory(path!, sort.field, sort.order, limit, offset).then((l) => l.files),
    !!path,
  );

  const selected: FileRecord[] = files.files.filter((f) => selection.has(f.id));
  const currentLocation = scanLocations.find((l) => path?.startsWith(l.path));
  const maxDir = Math.max(0, ...(listing.data?.subdirectories.map((d) => d.totalSize) ?? [0]));

  if (locations.isLoading) return <LoadingState />;
  if (scanLocations.length === 0) {
    return (
      <div className="p-7">
        <Card>
          <EmptyState
            icon={FolderTree}
            title="No folders indexed yet"
            description="Add a folder to browse it here. SORTD only reads metadata while indexing."
            action={
              <Button variant="primary" onClick={() => add.mutate()} loading={add.isPending}>
                <FolderPlus /> Add a folder
              </Button>
            }
          />
        </Card>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0">
      <aside
        className="flex w-[220px] shrink-0 flex-col border-r border-border"
        aria-label="Locations"
      >
        <div className="flex items-center justify-between px-4 pt-4 pb-2">
          <span className="text-[11px] font-medium tracking-wide text-subtle uppercase">
            Locations
          </span>
          <Tooltip content="Add folder">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Add folder"
              onClick={() => add.mutate()}
            >
              <FolderPlus />
            </Button>
          </Tooltip>
        </div>
        <div className="flex-1 overflow-auto px-2 pb-3">
          {scanLocations.map((l) => (
            <button
              key={l.id}
              onClick={() => go(l.path)}
              disabled={!l.lastScanId}
              title={l.path}
              className={cn(
                "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[13px] text-muted hover:bg-surface-2 hover:text-fg disabled:opacity-50",
                currentLocation?.id === l.id && "bg-surface-2 text-fg",
              )}
            >
              <HardDrive className="size-4 shrink-0" strokeWidth={1.75} />
              <span className="truncate">
                {l.path.split(/[\\/]/).filter(Boolean).pop() ?? l.path}
              </span>
              {!l.lastScanId && <span className="ml-auto text-[11px]">not scanned</span>}
            </button>
          ))}
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col gap-4 overflow-hidden px-6 pt-5 pb-5">
        {!path ? (
          <Card>
            <EmptyState
              icon={RefreshCw}
              title="Waiting for the first scan"
              description="This location hasn’t been scanned yet."
              action={
                <Button
                  variant="primary"
                  onClick={() => scan.mutate(undefined)}
                  disabled={jobs.scanRunning}
                >
                  Scan now
                </Button>
              }
            />
          </Card>
        ) : listing.isError ? (
          <Card>
            <ErrorState error={listing.error} onRetry={() => listing.refetch()} />
          </Card>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <nav
                aria-label="Breadcrumb"
                className="flex min-w-0 flex-wrap items-center gap-0.5 text-[13px]"
              >
                {listing.data?.breadcrumbs.map((b, i, arr) => (
                  <React.Fragment key={b.path}>
                    {i > 0 && <ChevronRight className="size-3.5 shrink-0 text-subtle" />}
                    <button
                      onClick={() => go(b.path)}
                      className={cn(
                        "truncate rounded px-1.5 py-0.5 hover:bg-surface-2",
                        i === arr.length - 1 ? "font-semibold" : "text-muted",
                      )}
                      aria-current={i === arr.length - 1 ? "page" : undefined}
                    >
                      {b.name}
                    </button>
                  </React.Fragment>
                ))}
              </nav>
              <div className="flex items-center gap-2">
                {listing.data && (
                  <span className="tabular text-[12px] text-muted">
                    {!listing.data.directory.complete && "≥ "}
                    {formatBytes(listing.data.directory.totalSize)} ·{" "}
                    {formatCount(listing.data.directory.totalFiles)} files
                    {listing.data.directory.modifiedAt
                      ? ` · modified ${formatRelative(listing.data.directory.modifiedAt)}`
                      : ""}
                  </span>
                )}
                <Button size="sm" onClick={() => setNewFolder(true)}>
                  <FolderPlus /> New folder
                </Button>
                <Button size="sm" onClick={() => api.revealDirectory(path).catch(() => {})}>
                  <FolderOpen /> Open
                </Button>
              </div>
            </div>

            {listing.data && !listing.data.directory.complete && (
              <div className="flex items-center gap-2 text-[12px] text-warning">
                <CircleAlert className="size-4" />
                Part of this folder couldn’t be read during the last scan, so totals are lower
                bounds.
              </div>
            )}

            {listing.data && listing.data.subdirectories.length > 0 && (
              <Card className="max-h-[38%] shrink-0 overflow-auto">
                <ul aria-label="Folders">
                  {listing.data.subdirectories.map((d) => (
                    <FolderRow key={d.path} d={d} max={maxDir} onOpen={() => go(d.path)} />
                  ))}
                </ul>
              </Card>
            )}

            {selection.size > 0 && (
              <div className="flex items-center gap-2 rounded-lg border border-accent/30 bg-accent-soft px-3 py-1.5 text-[12.5px]">
                <span className="tabular font-medium text-accent">{selection.size} selected</span>
                <div className="ml-auto flex gap-1.5">
                  <Button size="sm" onClick={() => actions.moveTo(selected)}>
                    <FolderInput /> Move
                  </Button>
                  <Button size="sm" onClick={() => actions.copyTo(selected)}>
                    <Copy /> Copy
                  </Button>
                  <Button size="sm" variant="danger-ghost" onClick={() => actions.trash(selected)}>
                    <Trash2 /> Trash
                  </Button>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label="Clear selection"
                    onClick={() => setSelection(new Set())}
                  >
                    <X />
                  </Button>
                </div>
              </div>
            )}

            <FileTable
              className="min-h-[200px] flex-1"
              files={files.files}
              total={files.total}
              isLoading={files.query.isLoading}
              error={files.query.error}
              onRetry={() => files.query.refetch()}
              hasMore={files.query.hasNextPage}
              onLoadMore={() => !files.query.isFetchingNextPage && void files.query.fetchNextPage()}
              sort={sort}
              onSort={setSort}
              showPath={false}
              selection={selection}
              onSelectionChange={setSelection}
              empty={
                <div className="px-5 py-10 text-center text-[13px] text-muted">
                  No files directly in this folder.
                </div>
              }
            />
            {listing.data && listing.data.directory.depth === 0 && currentLocation && (
              <div className="text-[11.5px] text-subtle">
                <Badge>Indexed view</Badge> Showing SORTD’s index of this folder. Rescan to pick up
                changes made outside SORTD.
              </div>
            )}
          </>
        )}
      </section>

      {newFolder && path && (
        <NameDialog
          title="New folder"
          initial=""
          confirmLabel="Review"
          onClose={() => setNewFolder(false)}
          onSubmit={(name) => {
            setNewFolder(false);
            actions.preview({
              label: `Create folder “${name}”`,
              origin: "manual",
              operations: [{ op: "createDirectory", parent: path, name }],
            });
          }}
        />
      )}
    </div>
  );
}
