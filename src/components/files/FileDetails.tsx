import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Cloud,
  Copy,
  ExternalLink,
  FolderInput,
  FolderOpen,
  Link2,
  Lock,
  Pencil,
  ShieldAlert,
  Trash2,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import { CATEGORIES, categoryMeta } from "@/lib/categories";
import { formatBytes, formatDate, formatPercent } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Sheet } from "@/components/ui/overlays";
import { Badge, Select, Separator, Tooltip } from "@/components/ui/primitives";
import { ErrorState, InlineNotice, LoadingState } from "@/components/common";
import { useToast } from "@/components/toast";
import { useFileActions } from "./FileActions";

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[110px_1fr] gap-3 py-1.5 text-[12.5px]">
      <dt className="text-muted">{label}</dt>
      <dd className="selectable min-w-0 break-words">{children}</dd>
    </div>
  );
}

export function FileDetailsSheet({
  fileId,
  onClose,
}: {
  fileId: number | null;
  onClose: () => void;
}) {
  return (
    <Sheet open={fileId !== null} onOpenChange={(o) => !o && onClose()} title="File details">
      {fileId !== null && <DetailsBody id={fileId} onClose={onClose} />}
    </Sheet>
  );
}

function DetailsBody({ id, onClose }: { id: number; onClose: () => void }) {
  const qc = useQueryClient();
  const toast = useToast();
  const actions = useFileActions();
  const q = useQuery({ queryKey: ["file", id], queryFn: () => api.fileDetails(id) });
  const override = useMutation({
    mutationFn: (category: string | null) => api.setCategoryOverride(id, category),
    onSuccess: () => {
      void qc.invalidateQueries();
      toast.success("Category updated");
    },
    onError: (e) => toast.error("Couldn’t change the category", errorMessage(e)),
  });

  if (q.isLoading) return <LoadingState />;
  if (q.isError || !q.data) return <ErrorState error={q.error} onRetry={() => q.refetch()} />;
  const { file, classification, signature, exists, unsafeToOpen, duplicates } = q.data;
  const meta = categoryMeta(classification.category);
  const Icon = meta.icon;
  const act = (fn: () => void) => () => {
    onClose();
    fn();
  };

  return (
    <div className="flex flex-col gap-4 p-5">
      <div className="flex items-start gap-3">
        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
          <Icon className="size-5" strokeWidth={1.75} />
        </div>
        <div className="min-w-0">
          <div className="selectable text-[14px] font-semibold break-words">{file.name}</div>
          <div className="mt-0.5 flex flex-wrap gap-1.5">
            <Badge>{formatBytes(file.size)}</Badge>
            {file.isHidden && <Badge>Hidden</Badge>}
            {file.isReadonly && (
              <Badge>
                <Lock /> Read-only
              </Badge>
            )}
            {file.isSymlink && (
              <Badge tone="violet">
                <Link2 /> Symbolic link
              </Badge>
            )}
            {file.isPlaceholder && (
              <Badge tone="violet">
                <Cloud /> Cloud placeholder
              </Badge>
            )}
          </div>
        </div>
      </div>

      {!exists && (
        <InlineNotice tone="warning" icon={ShieldAlert}>
          This file no longer exists at its indexed location. Rescan the location to update the
          index.
        </InlineNotice>
      )}

      <div className="flex flex-wrap gap-1.5">
        <Tooltip
          content={unsafeToOpen ? "SORTD never opens programs, scripts or links" : undefined}
        >
          <span>
            <Button size="sm" disabled={!exists || unsafeToOpen} onClick={() => actions.open(file)}>
              <ExternalLink /> Open
            </Button>
          </span>
        </Tooltip>
        <Button size="sm" disabled={!exists} onClick={() => actions.reveal(file)}>
          <FolderOpen /> Show in folder
        </Button>
        <Button size="sm" disabled={!exists} onClick={act(() => actions.rename(file))}>
          <Pencil /> Rename
        </Button>
        <Button size="sm" disabled={!exists} onClick={act(() => actions.moveTo([file]))}>
          <FolderInput /> Move
        </Button>
        <Button
          size="sm"
          disabled={!exists || file.isSymlink}
          onClick={act(() => actions.copyTo([file]))}
        >
          <Copy /> Copy
        </Button>
        <Button
          size="sm"
          variant="danger-ghost"
          disabled={!exists}
          onClick={act(() => actions.trash([file]))}
        >
          <Trash2 /> Trash
        </Button>
      </div>

      <Separator />

      <section>
        <h4 className="mb-2 text-[12px] font-semibold tracking-wide text-subtle uppercase">
          Classification
        </h4>
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-[13px]">
            <span className="font-medium">{meta.label}</span>
            <Badge
              tone={
                classification.confidence >= 0.9
                  ? "success"
                  : classification.confidence >= 0.6
                    ? "accent"
                    : "warning"
              }
            >
              {formatPercent(classification.confidence)} confidence
            </Badge>
          </div>
          <Select
            className="h-7"
            aria-label="Override category"
            value={file.categoryOverridden ? file.category : ""}
            disabled={override.isPending}
            onChange={(e) => override.mutate(e.target.value || null)}
          >
            <option value="">Automatic</option>
            {CATEGORIES.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </Select>
        </div>
        <ul className="mt-2 space-y-1 text-[12.5px] text-muted">
          {classification.reasons.map((r, i) => (
            <li key={i} className="flex gap-2">
              <span className="mt-[7px] size-1 shrink-0 rounded-full bg-subtle" />
              {r}
            </li>
          ))}
        </ul>
      </section>

      <Separator />

      <dl>
        <Row label="Location">{file.parent}</Row>
        <Row label="Modified">{formatDate(file.modifiedAt, true)}</Row>
        <Row label="Created">
          {file.createdAt ? formatDate(file.createdAt, true) : "Not reported by this filesystem"}
        </Row>
        <Row label="Type hint">
          {file.mime ?? "—"} <span className="text-subtle">(from extension)</span>
        </Row>
        {signature && (
          <Row label="Signature">
            {signature.mime} <span className="text-subtle">(from content)</span>
          </Row>
        )}
        <Row label="BLAKE3">
          {file.fullHash ? (
            <span className="font-mono text-[11.5px]">{file.fullHash}</span>
          ) : (
            <span className="text-muted">Not hashed (only potential duplicates are hashed)</span>
          )}
        </Row>
      </dl>

      {duplicates.length > 0 && (
        <>
          <Separator />
          <section>
            <h4 className="mb-2 text-[12px] font-semibold tracking-wide text-subtle uppercase">
              Identical copies ({duplicates.length})
            </h4>
            <ul className="space-y-1">
              {duplicates.map((d) => (
                <li key={d.id}>
                  <button
                    className="selectable w-full truncate rounded px-1.5 py-1 text-left text-[12.5px] text-muted hover:bg-surface-2 hover:text-fg"
                    onClick={() => actions.details(d)}
                    title={d.path}
                  >
                    {d.path}
                  </button>
                </li>
              ))}
            </ul>
          </section>
        </>
      )}
    </div>
  );
}
