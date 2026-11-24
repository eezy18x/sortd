import * as React from "react";
import { useInfiniteQuery, useMutation } from "@tanstack/react-query";
import { Copy, FolderOpen, Info, ScanSearch, ShieldCheck, Square, Trash2 } from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type { DuplicateGroup, DuplicateProgress, FileRecord } from "@/types";
import { categoryMeta } from "@/lib/categories";
import { formatBytes, formatCount, formatDate } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge, Card, Checkbox, Progress, Select } from "@/components/ui/primitives";
import {
  EmptyState,
  ErrorState,
  InlineNotice,
  LoadingState,
  Page,
  PageHeader,
  StatTile,
} from "@/components/common";
import { useFileActions } from "@/components/files/FileActions";
import { useToast } from "@/components/toast";
import { useJobs } from "@/hooks/useJobs";

const PHASES: Record<DuplicateProgress["phase"], string> = {
  grouping: "Grouping files by size",
  partial: "Comparing file samples",
  full: "Verifying full contents (BLAKE3)",
  done: "Done",
};

function ProgressCard({ p }: { p: DuplicateProgress }) {
  const frac =
    p.bytesToHash > 0 ? p.bytesHashed / p.bytesToHash : p.toHash > 0 ? p.hashed / p.toHash : 0;
  return (
    <Card className="px-5 py-4">
      <div className="flex items-center justify-between">
        <div className="text-[13.5px] font-medium">{PHASES[p.phase]}</div>
        <Button size="sm" onClick={() => void api.cancelDuplicateScan()}>
          <Square className="size-3" /> Stop
        </Button>
      </div>
      <Progress
        value={frac}
        indeterminate={p.phase === "grouping"}
        className="mt-3"
        label="Duplicate detection progress"
      />
      <div className="tabular mt-2.5 flex flex-wrap gap-x-6 text-[12.5px] text-muted">
        <span>{formatCount(p.candidates)} candidates (same size)</span>
        {p.toHash > 0 && (
          <span>
            {formatCount(p.hashed)} / {formatCount(p.toHash)} hashed
          </span>
        )}
        {p.bytesToHash > 0 && (
          <span>
            {formatBytes(p.bytesHashed)} / {formatBytes(p.bytesToHash)}
          </span>
        )}
        {p.changed > 0 && (
          <span className="text-warning">{p.changed} changed since scan (skipped)</span>
        )}
      </div>
    </Card>
  );
}

function GroupCard({
  group,
  included,
  keepId,
  onInclude,
  onKeep,
}: {
  group: DuplicateGroup;
  included: boolean;
  keepId: number;
  onInclude: (on: boolean) => void;
  onKeep: (id: number) => void;
}) {
  const actions = useFileActions();
  const Icon = categoryMeta(group.files[0]?.category ?? "other").icon;
  return (
    <Card className={cn(included && "border-accent/40")}>
      <div className="flex items-center gap-3 border-b border-border px-4 py-2.5">
        <Checkbox
          checked={included}
          onCheckedChange={onInclude}
          aria-label={`Include group of ${group.files[0]?.name}`}
        />
        <Icon className="size-4 text-muted" strokeWidth={1.75} />
        <span className="truncate text-[13px] font-medium">{group.files[0]?.name}</span>
        <Badge>
          {group.count} copies × {formatBytes(group.size)}
        </Badge>
        <span className="tabular ml-auto shrink-0 text-[12.5px] text-muted">
          {formatBytes(group.reclaimable)} reclaimable
        </span>
      </div>
      <ul role="radiogroup" aria-label="Copy to keep">
        {group.files.map((f: FileRecord) => {
          const keep = f.id === keepId;
          return (
            <li
              key={f.id}
              className="flex items-center gap-3 border-b border-border/60 px-4 py-2 last:border-0"
            >
              <label className="flex shrink-0 cursor-default items-center gap-2 text-[12px]">
                <input
                  type="radio"
                  name={`keep-${group.hash}`}
                  checked={keep}
                  onChange={() => onKeep(f.id)}
                  className="accent-[var(--accent)]"
                  aria-label={`Keep ${f.path}`}
                />
                <span
                  className={cn(
                    "w-12",
                    keep ? "font-medium text-success" : included ? "text-danger" : "text-subtle",
                  )}
                >
                  {keep ? "Keep" : included ? "Trash" : "—"}
                </span>
              </label>
              <button
                className="selectable min-w-0 flex-1 truncate text-left text-[12.5px] hover:underline"
                title={f.path}
                onClick={() => actions.details(f)}
              >
                {f.path}
              </button>
              <span className="shrink-0 text-[12px] text-subtle">{formatDate(f.modifiedAt)}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Show ${f.name} in folder`}
                onClick={() => actions.reveal(f)}
              >
                <FolderOpen />
              </Button>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}

export function DuplicatesPage() {
  const jobs = useJobs();
  const toast = useToast();
  const actions = useFileActions();
  const [sort, setSort] = React.useState("waste");
  const [included, setIncluded] = React.useState<Set<string>>(new Set());
  const [keep, setKeep] = React.useState<Map<string, number>>(new Map());

  const groups = useInfiniteQuery({
    queryKey: ["duplicates", sort],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => api.duplicateGroups(sort, 50, pageParam),
    getNextPageParam: (last) =>
      last.offset + last.items.length < last.total ? last.offset + last.items.length : undefined,
  });
  const all = React.useMemo(() => groups.data?.pages.flatMap((p) => p.items) ?? [], [groups.data]);
  const total = groups.data?.pages[0]?.total ?? 0;
  const reclaimable = all.reduce((s, g) => s + g.reclaimable, 0);

  const start = useMutation({
    mutationFn: api.startDuplicateScan,
    onError: (e) => toast.error("Couldn’t start duplicate detection", errorMessage(e)),
  });

  // Default keep: the oldest copy (first in the list).
  const keepFor = (g: DuplicateGroup) => keep.get(g.hash) ?? g.files[0]!.id;
  const toTrash = all
    .filter((g) => included.has(g.hash))
    .flatMap((g) => g.files.filter((f) => f.id !== keepFor(g)));
  const trashBytes = toTrash.reduce((s, f) => s + f.size, 0);

  const review = () =>
    actions.preview(
      {
        label: `Remove ${toTrash.length} duplicate${toTrash.length === 1 ? "" : "s"}`,
        origin: "duplicates",
        operations: toTrash.map((f) => ({ op: "trash", source: f.path })),
      },
      () => setIncluded(new Set()),
    );

  return (
    <Page>
      <PageHeader
        title="Duplicates"
        description="Exact duplicates, confirmed by comparing full file contents with BLAKE3 — never by name or size alone. SORTD never removes anything automatically."
        actions={
          <Button
            variant="primary"
            onClick={() => start.mutate()}
            disabled={jobs.duplicatesRunning || jobs.scanRunning}
            loading={start.isPending}
          >
            <ScanSearch /> {total > 0 ? "Re-check duplicates" : "Find duplicates"}
          </Button>
        }
      />

      {jobs.duplicatesRunning && jobs.duplicates && <ProgressCard p={jobs.duplicates} />}

      <div className="grid grid-cols-3 gap-3">
        <StatTile label="Duplicate groups" value={formatCount(total)} icon={Copy} />
        <StatTile
          label="Reclaimable (loaded groups)"
          value={formatBytes(reclaimable)}
          hint="Keeping one copy of each"
        />
        <StatTile
          label="Selected to trash"
          value={formatBytes(trashBytes)}
          hint={`${toTrash.length} files`}
          icon={Trash2}
        />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-[12.5px] text-muted">
          Sort by
          <Select value={sort} onChange={(e) => setSort(e.target.value)} className="h-7">
            <option value="waste">Most reclaimable</option>
            <option value="size">Largest files</option>
            <option value="count">Most copies</option>
          </Select>
        </label>
        <Button
          size="sm"
          variant="ghost"
          disabled={!all.length}
          onClick={() => setIncluded(new Set(all.map((g) => g.hash)))}
        >
          Select all loaded
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={!included.size}
          onClick={() => setIncluded(new Set())}
        >
          Clear
        </Button>
        <Button
          size="sm"
          variant="danger"
          className="ml-auto"
          disabled={!toTrash.length || jobs.operationRunning}
          onClick={review}
        >
          <Trash2 /> Review {toTrash.length || ""} for trash
        </Button>
      </div>

      <InlineNotice icon={ShieldCheck}>
        For each group you include, every copy except the one marked <strong>Keep</strong> goes to
        the system trash after you review the plan. Files changed since they were hashed are refused
        at execution time.
      </InlineNotice>

      {groups.isLoading ? (
        <LoadingState />
      ) : groups.isError ? (
        <ErrorState error={groups.error} onRetry={() => groups.refetch()} />
      ) : all.length === 0 ? (
        <Card>
          <EmptyState
            icon={Copy}
            title={jobs.duplicatesRunning ? "Looking for duplicates…" : "No duplicates found"}
            description={
              jobs.duplicatesRunning
                ? "Results appear here when the check finishes."
                : "Run a check after scanning. Only files that share an exact size are read, and only as much as needed."
            }
          />
        </Card>
      ) : (
        <div className="flex flex-col gap-3">
          {all.map((g) => (
            <GroupCard
              key={g.hash}
              group={g}
              included={included.has(g.hash)}
              keepId={keepFor(g)}
              onInclude={(on) => {
                const next = new Set(included);
                if (on) next.add(g.hash);
                else next.delete(g.hash);
                setIncluded(next);
              }}
              onKeep={(id) => setKeep(new Map(keep).set(g.hash, id))}
            />
          ))}
          {groups.hasNextPage && (
            <Button
              className="self-center"
              onClick={() => void groups.fetchNextPage()}
              loading={groups.isFetchingNextPage}
            >
              Load more groups
            </Button>
          )}
        </div>
      )}
      <div className="flex items-center gap-2 text-[12px] text-subtle">
        <Info className="size-3.5" /> Similar-looking images (perceptual matching) are a planned,
        separate feature and never mixed with exact duplicates.
      </div>
    </Page>
  );
}
