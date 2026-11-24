import * as React from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDown,
  ArrowUp,
  Cloud,
  Copy,
  Ellipsis,
  ExternalLink,
  FolderInput,
  FolderOpen,
  Info,
  Link2,
  Pencil,
  Trash2,
} from "lucide-react";
import type { FileRecord, Page, SortField, SortOrder } from "@/types";
import { categoryMeta } from "@/lib/categories";
import { formatBytes, formatDate, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Menu, MenuContent, MenuItem, MenuSeparator, MenuTrigger } from "@/components/ui/overlays";
import { Checkbox } from "@/components/ui/primitives";
import { ErrorState, LoadingState } from "@/components/common";
import { useFileActions } from "./FileActions";

const PAGE = 200;

/** Infinite, paginated file query. `fetchPage(offset, limit)` hits the backend. */
export function useInfiniteFiles(
  key: unknown[],
  fetchPage: (offset: number, limit: number) => Promise<Page<FileRecord>>,
  enabled = true,
) {
  const q = useInfiniteQuery({
    queryKey: key,
    enabled,
    initialPageParam: 0,
    queryFn: ({ pageParam }) => fetchPage(pageParam, PAGE),
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total ? last.offset + last.items.length : undefined,
  });
  const files = React.useMemo(() => q.data?.pages.flatMap((p) => p.items) ?? [], [q.data]);
  const total = q.data?.pages[0]?.total ?? 0;
  return { query: q, files, total };
}

export interface SortState {
  field: SortField;
  order: SortOrder;
}

function SortHeader({
  label,
  field,
  sort,
  onSort,
  className,
}: {
  label: string;
  field: SortField;
  sort?: SortState;
  onSort?: (s: SortState) => void;
  className?: string;
}) {
  const active = sort?.field === field;
  if (!onSort) return <div className={className}>{label}</div>;
  return (
    <button
      className={cn("flex items-center gap-1 hover:text-fg", active && "text-fg", className)}
      onClick={() => onSort({ field, order: active && sort?.order === "desc" ? "asc" : "desc" })}
      aria-sort={active ? (sort?.order === "asc" ? "ascending" : "descending") : "none"}
    >
      {label}
      {active &&
        (sort?.order === "asc" ? <ArrowUp className="size-3" /> : <ArrowDown className="size-3" />)}
    </button>
  );
}

export function FileRowMenu({ file }: { file: FileRecord }) {
  const a = useFileActions();
  return (
    <Menu>
      <MenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Actions for ${file.name}`}
          onClick={(e) => e.stopPropagation()}
        >
          <Ellipsis />
        </Button>
      </MenuTrigger>
      <MenuContent>
        <MenuItem icon={<Info />} onSelect={() => a.details(file)}>
          Details
        </MenuItem>
        <MenuItem icon={<ExternalLink />} onSelect={() => a.open(file)} disabled={file.isSymlink}>
          Open
        </MenuItem>
        <MenuItem icon={<FolderOpen />} onSelect={() => a.reveal(file)}>
          Show in folder
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={<Pencil />} onSelect={() => a.rename(file)}>
          Rename…
        </MenuItem>
        <MenuItem icon={<FolderInput />} onSelect={() => a.moveTo([file])}>
          Move to…
        </MenuItem>
        <MenuItem icon={<Copy />} onSelect={() => a.copyTo([file])} disabled={file.isSymlink}>
          Copy to…
        </MenuItem>
        <MenuSeparator />
        <MenuItem icon={<Trash2 />} danger onSelect={() => a.trash([file])}>
          Move to trash…
        </MenuItem>
      </MenuContent>
    </Menu>
  );
}

export interface FileTableProps {
  files: FileRecord[];
  total?: number;
  isLoading?: boolean;
  error?: unknown;
  onRetry?: () => void;
  hasMore?: boolean;
  onLoadMore?: () => void;
  sort?: SortState;
  onSort?: (s: SortState) => void;
  showPath?: boolean;
  selection?: Set<number>;
  onSelectionChange?: (s: Set<number>) => void;
  empty?: React.ReactNode;
  className?: string;
  dateField?: "modified" | "created";
}

const ROW_HEIGHT = 44;

export function FileTable({
  files,
  total,
  isLoading,
  error,
  onRetry,
  hasMore,
  onLoadMore,
  sort,
  onSort,
  showPath = true,
  selection,
  onSelectionChange,
  empty,
  className,
  dateField = "modified",
}: FileTableProps) {
  const actions = useFileActions();
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: files.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();
  const lastIndex = items[items.length - 1]?.index ?? 0;

  React.useEffect(() => {
    if (hasMore && onLoadMore && lastIndex >= files.length - 20) onLoadMore();
  }, [lastIndex, files.length, hasMore, onLoadMore]);

  const selectable = !!selection && !!onSelectionChange;
  const allSelected = selectable && files.length > 0 && files.every((f) => selection!.has(f.id));
  const someSelected = selectable && files.some((f) => selection!.has(f.id));
  const toggle = (id: number, on: boolean) => {
    if (!selection || !onSelectionChange) return;
    const next = new Set(selection);
    if (on) next.add(id);
    else next.delete(id);
    onSelectionChange(next);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    const idx = Number(target.dataset.index);
    if (Number.isNaN(idx)) return;
    const focusRow = (i: number) => {
      virtualizer.scrollToIndex(i);
      requestAnimationFrame(() =>
        scrollRef.current?.querySelector<HTMLElement>(`[data-index="${i}"]`)?.focus(),
      );
    };
    if (e.key === "ArrowDown") {
      e.preventDefault();
      focusRow(Math.min(files.length - 1, idx + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      focusRow(Math.max(0, idx - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const f = files[idx];
      if (f) actions.details(f);
    } else if (e.key === " " && selectable) {
      e.preventDefault();
      const f = files[idx];
      if (f) toggle(f.id, !selection!.has(f.id));
    }
  };

  const grid = cn(
    "grid items-center gap-3 px-4",
    selectable
      ? "grid-cols-[20px_minmax(0,1fr)_120px_88px_120px_32px]"
      : "grid-cols-[minmax(0,1fr)_120px_88px_120px_32px]",
  );

  return (
    <div
      className={cn(
        "flex min-h-0 flex-col overflow-hidden rounded-xl border border-border bg-surface",
        className,
      )}
    >
      <div
        className={cn(
          grid,
          "h-9 shrink-0 border-b border-border text-[11.5px] font-medium text-muted",
        )}
        role="row"
      >
        {selectable && (
          <Checkbox
            aria-label="Select all loaded files"
            checked={allSelected ? true : someSelected ? "indeterminate" : false}
            onCheckedChange={(on) =>
              onSelectionChange!(on ? new Set(files.map((f) => f.id)) : new Set())
            }
          />
        )}
        <SortHeader label="Name" field="name" sort={sort} onSort={onSort} />
        <div>Category</div>
        <SortHeader
          label="Size"
          field="size"
          sort={sort}
          onSort={onSort}
          className="justify-end text-right"
        />
        <SortHeader
          label={dateField === "created" ? "Created" : "Modified"}
          field={dateField}
          sort={sort}
          onSort={onSort}
        />
        <div />
      </div>

      {error ? (
        <ErrorState error={error} onRetry={onRetry} />
      ) : isLoading ? (
        <LoadingState />
      ) : files.length === 0 ? (
        empty
      ) : (
        <div
          ref={scrollRef}
          className="min-h-0 flex-1 overflow-auto"
          onKeyDown={onKeyDown}
          role="grid"
          aria-rowcount={total ?? files.length}
        >
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((vi) => {
              const f = files[vi.index]!;
              const meta = categoryMeta(f.category);
              const Icon = meta.icon;
              const selected = selection?.has(f.id) ?? false;
              const date = dateField === "created" ? f.createdAt : f.modifiedAt;
              return (
                <div
                  key={f.id}
                  data-index={vi.index}
                  tabIndex={0}
                  role="row"
                  aria-selected={selectable ? selected : undefined}
                  onClick={() => actions.details(f)}
                  onDoubleClick={() => actions.open(f)}
                  className={cn(
                    grid,
                    "absolute inset-x-0 border-b border-border/60 text-[13px] outline-none hover:bg-surface-2 focus-visible:bg-surface-2 focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent",
                    selected && "bg-accent-soft/60 hover:bg-accent-soft",
                  )}
                  style={{ height: ROW_HEIGHT, transform: `translateY(${vi.start}px)` }}
                >
                  {selectable && (
                    <span onClick={(e) => e.stopPropagation()}>
                      <Checkbox
                        checked={selected}
                        onCheckedChange={(on) => toggle(f.id, on)}
                        aria-label={`Select ${f.name}`}
                      />
                    </span>
                  )}
                  <div className="flex min-w-0 items-center gap-2.5">
                    <Icon className="size-4 shrink-0 text-muted" strokeWidth={1.75} />
                    <div className="min-w-0">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate">{f.name}</span>
                        {f.isSymlink && (
                          <Link2
                            className="size-3.5 shrink-0 text-violet"
                            aria-label="Symbolic link"
                          />
                        )}
                        {f.isPlaceholder && (
                          <Cloud
                            className="size-3.5 shrink-0 text-violet"
                            aria-label="Cloud placeholder"
                          />
                        )}
                      </div>
                      {showPath && (
                        <div className="truncate text-[11.5px] text-subtle" title={f.parent}>
                          {f.parent}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="truncate text-[12.5px] text-muted">{meta.label}</div>
                  <div className="tabular text-right text-[12.5px]">{formatBytes(f.size)}</div>
                  <div className="truncate text-[12.5px] text-muted" title={formatDate(date, true)}>
                    {formatRelative(date)}
                  </div>
                  <div onClick={(e) => e.stopPropagation()}>
                    <FileRowMenu file={f} />
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}
      {total !== undefined && files.length > 0 && (
        <div className="flex h-8 shrink-0 items-center justify-between border-t border-border px-4 text-[11.5px] text-subtle">
          <span className="tabular">
            {files.length < total
              ? `Showing ${files.length.toLocaleString()} of ${total.toLocaleString()}`
              : `${total.toLocaleString()} files`}
          </span>
          {selectable && selection!.size > 0 && (
            <span className="tabular">{selection!.size.toLocaleString()} selected</span>
          )}
        </div>
      )}
    </div>
  );
}
