import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowRight,
  CircleAlert,
  CircleCheck,
  CircleSlash,
  Copy,
  FolderPlus,
  FolderInput,
  HardDrive,
  Pencil,
  ShieldCheck,
  Trash2,
  Undo2,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type {
  Batch,
  ConflictPolicy,
  DryRunItem,
  OperationItem,
  OpKind,
  PlanRequest,
} from "@/types";
import { formatBytes, splitPath } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/overlays";
import { Badge, Checkbox, Progress, Select } from "@/components/ui/primitives";
import { InlineNotice, LoadingState } from "@/components/common";
import { useToast } from "@/components/toast";
import { useJobs } from "@/hooks/useJobs";

const OP_META: Record<OpKind, { label: string; icon: typeof Copy }> = {
  move: { label: "Move", icon: FolderInput },
  rename: { label: "Rename", icon: Pencil },
  copy: { label: "Copy", icon: Copy },
  mkdir: { label: "New folder", icon: FolderPlus },
  trash: { label: "Trash", icon: Trash2 },
};

function statusBadge(item: OperationItem, dry?: DryRunItem) {
  if (dry) {
    return dry.ok ? (
      <Badge tone="success">
        <CircleCheck /> Ready
      </Badge>
    ) : (
      <Badge tone="danger">
        <CircleAlert /> Would fail
      </Badge>
    );
  }
  switch (item.status) {
    case "pending":
      return <Badge tone="accent">Planned</Badge>;
    case "skipped":
      return (
        <Badge tone="warning">
          <CircleSlash /> Skipped
        </Badge>
      );
    case "done":
      return (
        <Badge tone="success">
          <CircleCheck /> {item.verified ? "Done · verified" : "Done"}
        </Badge>
      );
    case "failed":
    case "interrupted":
      return (
        <Badge tone="danger">
          <CircleAlert /> {item.status === "failed" ? "Failed" : "Interrupted"}
        </Badge>
      );
    case "undone":
      return <Badge>Undone</Badge>;
    case "undo_failed":
      return <Badge tone="danger">Undo failed</Badge>;
    default:
      return <Badge>{item.status}</Badge>;
  }
}

export function ItemRow({ item, dry }: { item: OperationItem; dry?: DryRunItem }) {
  const meta = OP_META[item.op];
  const Icon = meta.icon;
  const src = item.source ? splitPath(item.source) : null;
  const dst = item.destination ? splitPath(item.destination) : null;
  const note = dry?.message ?? item.error ?? item.issue;
  const noteTone =
    dry && !dry.ok
      ? "text-danger"
      : item.error
        ? "text-danger"
        : item.status === "skipped"
          ? "text-warning"
          : "text-subtle";
  return (
    <li className="flex gap-3 border-b border-border px-5 py-2.5 last:border-b-0">
      <Icon
        className="mt-0.5 size-4 shrink-0 text-muted"
        strokeWidth={1.75}
        aria-label={meta.label}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-[13px]">
          {src && <span className="truncate font-medium">{src.name}</span>}
          {src && dst && <ArrowRight className="size-3.5 shrink-0 text-subtle" />}
          {dst && (
            <span className={cn("truncate", src ? "text-muted" : "font-medium")}>
              {item.op === "rename" || item.op === "mkdir" ? dst.name : dst.dir}
            </span>
          )}
        </div>
        <div
          className="selectable mt-0.5 truncate text-[11.5px] text-subtle"
          title={item.source ?? item.destination ?? ""}
        >
          {item.op === "rename" ? src?.dir : item.op === "mkdir" ? dst?.dir : item.source}
        </div>
        {note && <div className={cn("mt-1 text-[12px]", noteTone)}>{note}</div>}
      </div>
      <div className="flex shrink-0 flex-col items-end gap-1">
        {statusBadge(item, dry)}
        {item.size != null && (
          <span className="tabular text-[11.5px] text-subtle">{formatBytes(item.size)}</span>
        )}
        {item.crossVolume && (
          <Badge tone="violet">
            <HardDrive /> Other drive
          </Badge>
        )}
      </div>
    </li>
  );
}

/**
 * Plans the request, shows a preview, optionally dry-runs it and executes it
 * only when the user explicitly confirms. Unexecuted plans are discarded on
 * close.
 */
export function OperationPreview({
  request,
  onClose,
  onExecuted,
}: {
  request: Omit<PlanRequest, "conflictPolicy" | "allowCrossVolume"> & {
    conflictPolicy?: ConflictPolicy;
  };
  onClose: () => void;
  onExecuted?: (batch: Batch) => void;
}) {
  const toast = useToast();
  const qc = useQueryClient();
  const jobs = useJobs();
  const [policy, setPolicy] = React.useState<ConflictPolicy>(request.conflictPolicy ?? "skip");
  const [allowCross, setAllowCross] = React.useState(false);
  const [result, setResult] = React.useState<Batch | null>(null);
  const [dry, setDry] = React.useState<Map<number, DryRunItem> | null>(null);
  const [busy, setBusy] = React.useState<"dry" | "exec" | "undo" | null>(null);
  const [phase, setPhase] = React.useState<"preview" | "result">("preview");
  const [instance] = React.useState(() => Math.random().toString(36).slice(2));
  const isTrash = request.operations.some((o) => o.op === "trash");

  // The plan is server state: one plan per (dialog, policy, cross-drive)
  // combination. Query de-duplication guarantees a single plan per key.
  const planQuery = useQuery({
    queryKey: ["plan", instance, policy, allowCross],
    queryFn: () =>
      api.planOperations({ ...request, conflictPolicy: policy, allowCrossVolume: allowCross }),
    staleTime: Infinity,
    gcTime: 0,
    retry: false,
    refetchOnMount: false,
  });
  const batch = result ?? planQuery.data ?? null;
  const error = planQuery.error;
  const planning = planQuery.isFetching;

  const changePlan = (nextPolicy: ConflictPolicy, nextAllowCross: boolean) => {
    const old = planQuery.data;
    if (old && old.summary.status === "planned")
      void api.discardPlan(old.summary.id).catch(() => {});
    setDry(null);
    setPolicy(nextPolicy);
    setAllowCross(nextAllowCross);
  };

  const close = () => {
    if (!result && batch && batch.summary.status === "planned")
      void api.discardPlan(batch.summary.id).catch(() => {});
    onClose();
  };

  const runDry = async () => {
    if (!batch) return;
    setBusy("dry");
    try {
      const res = await api.dryRun(batch.summary.id);
      setDry(new Map(res.map((r) => [r.itemId, r])));
      const failing = res.filter((r) => !r.ok).length;
      if (failing === 0)
        toast.success(
          "Dry run passed",
          `All ${res.length} operations are ready. Nothing was changed.`,
        );
      else
        toast.info(
          "Dry run finished",
          `${failing} of ${res.length} operations would fail. Nothing was changed.`,
        );
    } catch (e) {
      toast.error("Dry run failed", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const execute = async () => {
    if (!batch) return;
    setBusy("exec");
    try {
      const b = await api.executeOperations(batch.summary.id);
      setResult(b);
      setPhase("result");
      const s = b.summary;
      if (s.failed === 0)
        toast.success(
          `${request.label} complete`,
          `${s.succeeded} succeeded${s.skipped ? `, ${s.skipped} skipped` : ""}.`,
        );
      else
        toast.error(
          `${request.label} finished with problems`,
          `${s.succeeded} succeeded, ${s.failed} failed. Review the details.`,
        );
      void qc.invalidateQueries();
      onExecuted?.(b);
    } catch (e) {
      toast.error("Could not run the operations", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const undo = async () => {
    if (!batch) return;
    setBusy("undo");
    try {
      const b = await api.undoBatch(batch.summary.id);
      setResult(b);
      const failed = b.items.filter((i) => i.status === "undo_failed").length;
      if (failed === 0) toast.success("Undone", "Everything was put back where it was.");
      else toast.error("Partially undone", `${failed} item(s) could not be reversed; see details.`);
      void qc.invalidateQueries();
    } catch (e) {
      toast.error("Undo failed", errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  const pending = batch?.pending ?? 0;
  const skipped = batch?.items.filter((i) => i.status === "skipped").length ?? 0;
  const crossSkipped =
    batch?.items.filter((i) => i.crossVolume && i.status === "skipped").length ?? 0;
  const progress =
    jobs.operation && batch && jobs.operation.batchId === batch.summary.id ? jobs.operation : null;

  const footer =
    phase === "preview" ? (
      <>
        <div className="mr-auto flex items-center gap-2 text-[12px] text-muted">
          <ShieldCheck className="size-4 text-accent" />
          Nothing is changed until you confirm.
        </div>
        <Button variant="ghost" onClick={close}>
          Cancel
        </Button>
        <Button
          onClick={runDry}
          disabled={!batch || pending === 0 || busy !== null || planning}
          loading={busy === "dry"}
        >
          Dry run
        </Button>
        <Button
          variant={isTrash ? "danger" : "primary"}
          onClick={execute}
          disabled={!batch || pending === 0 || busy !== null || planning}
          loading={busy === "exec"}
        >
          {isTrash
            ? `Move ${pending} to trash`
            : `Run ${pending} operation${pending === 1 ? "" : "s"}`}
        </Button>
      </>
    ) : (
      <>
        {batch?.summary.canUndo && batch.summary.status !== "undone" && (
          <Button
            className="mr-auto"
            onClick={undo}
            loading={busy === "undo"}
            disabled={busy !== null}
          >
            <Undo2 /> Undo
          </Button>
        )}
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </>
    );

  return (
    <Dialog open onOpenChange={(o) => !o && busy !== "exec" && close()}>
      <DialogContent
        title={phase === "preview" ? `Review: ${request.label}` : `Result: ${request.label}`}
        description={
          phase === "preview"
            ? "Every change is listed below. Existing files are never overwritten."
            : "Each completed item was verified after it ran."
        }
        className="w-[min(760px,94vw)]"
        footer={footer}
        onInteractOutside={(e) => e.preventDefault()}
      >
        {error ? (
          <div className="p-5">
            <InlineNotice tone="danger" icon={CircleAlert}>
              {errorMessage(error)}
            </InlineNotice>
          </div>
        ) : !batch || planning ? (
          <LoadingState label="Planning…" />
        ) : (
          <div className="flex flex-col">
            <div className="flex flex-wrap items-center gap-x-5 gap-y-2 border-b border-border bg-surface-2/50 px-5 py-3 text-[12.5px]">
              <span>
                <span className="tabular font-semibold">{pending}</span>{" "}
                <span className="text-muted">to run</span>
              </span>
              {skipped > 0 && (
                <span>
                  <span className="tabular font-semibold text-warning">{skipped}</span>{" "}
                  <span className="text-muted">skipped</span>
                </span>
              )}
              {batch.totalBytes > 0 && (
                <span className="text-muted">{formatBytes(batch.totalBytes)} affected</span>
              )}
              {phase === "preview" && (
                <label className="ml-auto flex items-center gap-2 text-muted">
                  If a name is taken
                  <Select
                    value={policy}
                    onChange={(e) => changePlan(e.target.value as ConflictPolicy, allowCross)}
                    disabled={busy !== null || planning}
                    className="h-7"
                  >
                    <option value="skip">Skip the item</option>
                    <option value="rename">Keep both (rename)</option>
                  </Select>
                </label>
              )}
            </div>

            {phase === "preview" && (crossSkipped > 0 || allowCross) && (
              <div className="border-b border-border px-5 py-3">
                <label className="flex items-start gap-2.5 text-[12.5px]">
                  <Checkbox
                    checked={allowCross}
                    onCheckedChange={(v) => changePlan(policy, v)}
                    aria-label="Allow moves between drives"
                    className="mt-0.5"
                  />
                  <span>
                    <span className="font-medium">Allow moves between drives.</span>{" "}
                    <span className="text-muted">
                      Each file is copied, verified with a BLAKE3 checksum, and only then removed
                      from its original drive.
                    </span>
                  </span>
                </label>
              </div>
            )}

            {isTrash && phase === "preview" && (
              <div className="border-b border-border px-5 py-3">
                <InlineNotice tone="warning" icon={Trash2}>
                  Items go to the system trash, where you can restore them. SORTD never deletes
                  files permanently.
                </InlineNotice>
              </div>
            )}

            {progress && progress.phase !== "done" && (
              <div className="border-b border-border px-5 py-3">
                <div className="mb-1.5 flex justify-between text-[12px] text-muted">
                  <span className="truncate">{progress.current ?? "Working…"}</span>
                  <span className="tabular">
                    {progress.done} / {progress.total}
                  </span>
                </div>
                <Progress
                  value={progress.total ? progress.done / progress.total : 0}
                  label="Operation progress"
                />
              </div>
            )}

            <ul className="max-h-[46vh] overflow-auto" aria-label="Planned operations">
              {batch.items.map((it) => (
                <ItemRow key={it.id} item={it} dry={dry?.get(it.id)} />
              ))}
            </ul>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
