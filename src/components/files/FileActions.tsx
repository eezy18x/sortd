import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { api, errorMessage } from "@/services/api";
import type { FileRecord, OperationRequest } from "@/types";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/overlays";
import { Input } from "@/components/ui/primitives";
import { useToast } from "@/components/toast";
import { OperationPreview } from "@/components/operations/OperationPreview";
import { DirectoryPicker } from "@/components/operations/DirectoryPicker";
import { FileDetailsSheet } from "./FileDetails";
import { invalidNameReason } from "@/lib/validation";
import { FileActionsContext, type FileActionsApi, type PreviewRequest } from "./fileActionsContext";

export { useFileActions } from "./fileActionsContext";

function plural(n: number, word: string) {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

export function FileActionsProvider({ children }: { children: React.ReactNode }) {
  const toast = useToast();
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [detailsId, setDetailsId] = React.useState<number | null>(null);
  const [renaming, setRenaming] = React.useState<FileRecord | null>(null);
  const [picking, setPicking] = React.useState<{
    files: FileRecord[];
    mode: "move" | "copy";
  } | null>(null);
  const [preview, setPreview] = React.useState<{
    request: PreviewRequest;
    onExecuted?: () => void;
  } | null>(null);

  const conflictPolicy = settings.data?.defaultConflictPolicy ?? "skip";

  /** Single operations honour "confirm single operations"; bulk ones always preview. */
  const runOrPreview = React.useCallback(
    async (request: PreviewRequest) => {
      const single = request.operations.length === 1 && request.operations[0]!.op !== "trash";
      if (single && settings.data && !settings.data.confirmSingleOperations) {
        try {
          const plan = await api.planOperations({
            ...request,
            conflictPolicy,
            allowCrossVolume: false,
          });
          if (plan.pending === 0) {
            await api.discardPlan(plan.summary.id);
            toast.error(
              "Nothing was changed",
              plan.items.find((i) => i.issue)?.issue ?? "The operation was skipped.",
            );
            return;
          }
          const b = await api.executeOperations(plan.summary.id);
          if (b.summary.failed > 0)
            toast.error("Operation failed", b.items.find((i) => i.error)?.error ?? undefined);
          else toast.success(request.label, "Done. You can undo it from Activity History.");
        } catch (e) {
          toast.error("Operation failed", errorMessage(e));
        }
        return;
      }
      setPreview({ request });
    },
    [settings.data, conflictPolicy, toast],
  );

  const actions = React.useMemo<FileActionsApi>(
    () => ({
      details: (f) => setDetailsId(f.id),
      open: (f) =>
        api.openFile(f.id).catch((e) => toast.error("Couldn’t open the file", errorMessage(e))),
      reveal: (f) =>
        api.revealFile(f.id).catch((e) => toast.error("Couldn’t show the file", errorMessage(e))),
      rename: (f) => setRenaming(f),
      moveTo: (files) => files.length && setPicking({ files, mode: "move" }),
      copyTo: (files) => files.length && setPicking({ files, mode: "copy" }),
      trash: (files) => {
        if (!files.length) return;
        setPreview({
          request: {
            label:
              files.length === 1
                ? `Move “${files[0]!.name}” to trash`
                : `Move ${plural(files.length, "file")} to trash`,
            origin: "manual",
            operations: files.map((f) => ({ op: "trash", source: f.path })),
          },
        });
      },
      preview: (request, onExecuted) => setPreview({ request, onExecuted }),
    }),
    [toast],
  );

  return (
    <FileActionsContext.Provider value={actions}>
      {children}

      <FileDetailsSheet fileId={detailsId} onClose={() => setDetailsId(null)} />

      {renaming && (
        <RenameDialog
          file={renaming}
          onClose={() => setRenaming(null)}
          onSubmit={(newName) => {
            const f = renaming;
            setRenaming(null);
            void runOrPreview({
              label: `Rename “${f.name}”`,
              origin: "manual",
              operations: [{ op: "rename", source: f.path, newName }],
            });
          }}
        />
      )}

      {picking && (
        <DirectoryPicker
          title={
            picking.mode === "move"
              ? `Move ${plural(picking.files.length, "item")}`
              : `Copy ${plural(picking.files.length, "item")}`
          }
          confirmLabel={picking.mode === "move" ? "Review move" : "Review copy"}
          onClose={() => setPicking(null)}
          onPick={(dir) => {
            const { files, mode } = picking;
            setPicking(null);
            const operations: OperationRequest[] = files.map((f) =>
              mode === "move"
                ? { op: "move", source: f.path, destinationDir: dir }
                : { op: "copy", source: f.path, destinationDir: dir },
            );
            void runOrPreview({
              label: `${mode === "move" ? "Move" : "Copy"} ${files.length === 1 ? `“${files[0]!.name}”` : plural(files.length, "file")}`,
              origin: "manual",
              operations,
            });
          }}
        />
      )}

      {preview && (
        <OperationPreview
          request={{ ...preview.request, conflictPolicy }}
          onClose={() => setPreview(null)}
          onExecuted={() => preview.onExecuted?.()}
        />
      )}
    </FileActionsContext.Provider>
  );
}

function RenameDialog({
  file,
  onClose,
  onSubmit,
}: {
  file: FileRecord;
  onClose: () => void;
  onSubmit: (name: string) => void;
}) {
  const [name, setName] = React.useState(file.name);
  const inputRef = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    const dot = file.name.lastIndexOf(".");
    el.setSelectionRange(0, dot > 0 ? dot : file.name.length);
  }, [file.name]);
  const trimmed = name.trim();
  const reason = invalidNameReason(trimmed);
  const invalid = reason !== null;
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title="Rename"
        description={<span className="selectable">{file.path}</span>}
        footer={
          <>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={invalid || trimmed === file.name}
              onClick={() => onSubmit(trimmed)}
            >
              Rename
            </Button>
          </>
        }
      >
        <form
          className="p-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (!invalid && trimmed !== file.name) onSubmit(trimmed);
          }}
        >
          <Input
            ref={inputRef}
            value={name}
            onChange={(e) => setName(e.target.value)}
            aria-label="New name"
            aria-invalid={invalid}
          />
          {invalid && trimmed && <p className="mt-2 text-[12px] text-danger">{reason}</p>}
        </form>
      </DialogContent>
    </Dialog>
  );
}
