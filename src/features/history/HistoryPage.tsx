import * as React from "react";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, History, ListChecks, Undo2 } from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type { ActivityEntry, BatchSummary } from "@/types";
import { formatDate, formatRelative } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { ConfirmDialog, Dialog, DialogContent } from "@/components/ui/overlays";
import { Badge, Card, Select } from "@/components/ui/primitives";
import { EmptyState, ErrorState, LoadingState, Page, PageHeader } from "@/components/common";
import { ItemRow } from "@/components/operations/OperationPreview";
import { useToast } from "@/components/toast";

const STATUS_TONE: Record<string, "success" | "warning" | "danger" | "accent" | "neutral"> = {
  completed: "success",
  partial: "warning",
  failed: "danger",
  interrupted: "warning",
  running: "accent",
  undone: "neutral",
  planned: "neutral",
};

const KIND_GROUPS: { label: string; kinds: string[] }[] = [
  { label: "All activity", kinds: [] },
  {
    label: "Scans",
    kinds: [
      "scan_started",
      "scan_completed",
      "scan_cancelled",
      "scan_failed",
      "location_added",
      "location_removed",
    ],
  },
  {
    label: "File operations",
    kinds: [
      "file_moved",
      "file_renamed",
      "file_copied",
      "file_trashed",
      "directory_created",
      "operation_failed",
      "undo",
      "operation_executed",
      "recovery",
    ],
  },
  { label: "Previews & dry runs", kinds: ["operation_previewed", "operation_dry_run"] },
  {
    label: "Insights",
    kinds: ["duplicates_completed", "recommendations_generated", "category_overridden"],
  },
  { label: "Settings & maintenance", kinds: ["settings_changed", "index_reset"] },
];

function BatchDialog({ id, onClose }: { id: number; onClose: () => void }) {
  const q = useQuery({ queryKey: ["batch", id], queryFn: () => api.batch(id) });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={q.data?.summary.label ?? "Operation"}
        description={
          q.data
            ? `${formatDate(q.data.summary.executedAt ?? q.data.summary.createdAt, true)} · ${q.data.summary.status}`
            : undefined
        }
        className="w-[min(760px,94vw)]"
      >
        {q.isLoading ? (
          <LoadingState />
        ) : q.isError ? (
          <ErrorState error={q.error} />
        ) : (
          <ul className="max-h-[60vh] overflow-auto">
            {q.data?.items.map((it) => (
              <ItemRow key={it.id} item={it} />
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  );
}

function Operations() {
  const qc = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = React.useState<number | null>(null);
  const [confirm, setConfirm] = React.useState<BatchSummary | null>(null);
  const batches = useInfiniteQuery({
    queryKey: ["batches"],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => api.batches(50, pageParam),
    getNextPageParam: (last, all) => {
      const loaded = all.reduce((s, p) => s + p.items.length, 0);
      return loaded < last.total ? loaded : undefined;
    },
  });
  const undo = useMutation({
    mutationFn: (id: number) => api.undoBatch(id),
    onSuccess: (b) => {
      setConfirm(null);
      void qc.invalidateQueries();
      const failed = b.items.filter((i) => i.status === "undo_failed").length;
      if (failed)
        toast.error(
          "Partially undone",
          `${failed} item(s) could not be reversed. Open the batch for details.`,
        );
      else toast.success("Undone", b.summary.label);
    },
    onError: (e) => toast.error("Undo failed", errorMessage(e)),
  });

  const items = batches.data?.pages.flatMap((p) => p.items) ?? [];
  if (batches.isLoading) return <LoadingState />;
  if (batches.isError)
    return <ErrorState error={batches.error} onRetry={() => batches.refetch()} />;
  if (items.length === 0)
    return (
      <EmptyState
        icon={ListChecks}
        title="No file operations yet"
        description="Moves, renames, copies and trash operations you approve appear here, with undo where possible."
      />
    );

  return (
    <>
      <ul>
        {items.map((b) => (
          <li
            key={b.id}
            className="flex items-center gap-3 border-b border-border/60 px-4 py-3 last:border-0"
          >
            <button className="min-w-0 flex-1 text-left" onClick={() => setOpen(b.id)}>
              <div className="flex items-center gap-2">
                <span className="truncate text-[13px] font-medium hover:underline">{b.label}</span>
                <Badge tone={STATUS_TONE[b.status] ?? "neutral"}>{b.status}</Badge>
              </div>
              <div className="tabular mt-0.5 text-[12px] text-muted">
                {b.succeeded} done
                {b.failed > 0 && <span className="text-danger"> · {b.failed} failed</span>}
                {b.skipped > 0 && ` · ${b.skipped} skipped`} ·{" "}
                {formatRelative(b.executedAt ?? b.createdAt)}
              </div>
            </button>
            {b.canUndo && (
              <Button size="sm" onClick={() => setConfirm(b)}>
                <Undo2 /> Undo
              </Button>
            )}
          </li>
        ))}
      </ul>
      {batches.hasNextPage && (
        <div className="flex justify-center p-3">
          <Button
            size="sm"
            onClick={() => void batches.fetchNextPage()}
            loading={batches.isFetchingNextPage}
          >
            Load more
          </Button>
        </div>
      )}
      {open !== null && <BatchDialog id={open} onClose={() => setOpen(null)} />}
      <ConfirmDialog
        open={!!confirm}
        onOpenChange={(o) => !o && setConfirm(null)}
        title="Undo this operation?"
        description={
          <>
            SORTD will reverse “{confirm?.label}” in reverse order. Items that changed since, or
            whose original place is now occupied, are left untouched and reported. Items sent to the
            trash must be restored from the system trash.
          </>
        }
        confirmLabel="Undo"
        loading={undo.isPending}
        onConfirm={() => confirm && undo.mutate(confirm.id)}
      />
    </>
  );
}

function ActivityRow({ e }: { e: ActivityEntry }) {
  const [open, setOpen] = React.useState(false);
  const details = e.details
    ? Object.entries(e.details).filter(([, v]) => v !== null && v !== undefined)
    : [];
  return (
    <li className="border-b border-border/60 px-4 py-2.5 last:border-0">
      <div className="flex items-start gap-3">
        <span
          className="tabular w-[132px] shrink-0 pt-px text-[12px] text-subtle"
          title={new Date(e.tsMs).toLocaleString()}
        >
          {new Date(e.tsMs).toLocaleString(undefined, { dateStyle: "short", timeStyle: "medium" })}
        </span>
        <div className="min-w-0 flex-1">
          <div className="selectable text-[13px] break-words">{e.summary}</div>
          {open && details.length > 0 && (
            <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 rounded-lg bg-surface-2 px-3 py-2 text-[12px]">
              {details.map(([k, v]) => (
                <React.Fragment key={k}>
                  <dt className="text-muted">{k}</dt>
                  <dd className="selectable font-mono text-[11.5px] break-all">
                    {typeof v === "object" ? JSON.stringify(v) : String(v)}
                  </dd>
                </React.Fragment>
              ))}
            </dl>
          )}
        </div>
        <Badge className="shrink-0">{e.kind.replace(/_/g, " ")}</Badge>
        {details.length > 0 && (
          <button
            className="shrink-0 text-subtle hover:text-fg"
            onClick={() => setOpen((o) => !o)}
            aria-expanded={open}
            aria-label="Show details"
          >
            <ChevronDown className={cn("size-4 transition-transform", open && "rotate-180")} />
          </button>
        )}
      </div>
    </li>
  );
}

function ActivityLog() {
  const [group, setGroup] = React.useState(0);
  const kinds = KIND_GROUPS[group]!.kinds;
  const log = useInfiniteQuery({
    queryKey: ["activity", group],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => api.activity(kinds, 100, pageParam),
    getNextPageParam: (last, all) => {
      const loaded = all.reduce((s, p) => s + p.items.length, 0);
      return loaded < last.total ? loaded : undefined;
    },
  });
  const items = log.data?.pages.flatMap((p) => p.items) ?? [];
  return (
    <>
      <div className="border-b border-border px-4 py-2.5">
        <Select
          value={group}
          onChange={(e) => setGroup(Number(e.target.value))}
          aria-label="Filter activity"
          className="h-7"
        >
          {KIND_GROUPS.map((g, i) => (
            <option key={g.label} value={i}>
              {g.label}
            </option>
          ))}
        </Select>
      </div>
      {log.isLoading ? (
        <LoadingState />
      ) : log.isError ? (
        <ErrorState error={log.error} onRetry={() => log.refetch()} />
      ) : items.length === 0 ? (
        <EmptyState icon={History} title="No activity recorded" />
      ) : (
        <ul>
          {items.map((e) => (
            <ActivityRow key={e.id} e={e} />
          ))}
        </ul>
      )}
      {log.hasNextPage && (
        <div className="flex justify-center p-3">
          <Button
            size="sm"
            onClick={() => void log.fetchNextPage()}
            loading={log.isFetchingNextPage}
          >
            Load more
          </Button>
        </div>
      )}
    </>
  );
}

export function HistoryPage() {
  const [tab, setTab] = React.useState<"operations" | "activity">("operations");
  return (
    <Page>
      <PageHeader
        title="Activity History"
        description="A local record of scans, previews and every file operation SORTD performed. File contents are never logged."
      />
      <div
        className="flex rounded-lg border border-border bg-surface p-0.5 self-start"
        role="tablist"
      >
        {(
          [
            ["operations", "File operations"],
            ["activity", "Activity log"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={cn(
              "h-7 rounded-md px-3 text-[12.5px] text-muted",
              tab === id && "bg-surface-2 text-fg",
            )}
          >
            {label}
          </button>
        ))}
      </div>
      <Card>{tab === "operations" ? <Operations /> : <ActivityLog />}</Card>
    </Page>
  );
}
