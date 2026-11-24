import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronRight, Folder, FolderPlus, HardDrive } from "lucide-react";
import { api } from "@/services/api";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent } from "@/components/ui/overlays";
import { Input } from "@/components/ui/primitives";
import { ErrorState, LoadingState } from "@/components/common";
import { invalidSubpathReason, joinSubpath } from "@/lib/validation";

/**
 * Picks a destination folder among authorized locations, browsing the
 * index. A new subfolder name may be typed; SORTD creates it as part of the
 * previewed plan.
 */
export function DirectoryPicker({
  title,
  confirmLabel,
  onPick,
  onClose,
}: {
  title: string;
  confirmLabel: string;
  onPick: (path: string) => void;
  onClose: () => void;
}) {
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });
  const [current, setCurrent] = React.useState<string | null>(null);
  const [indexed, setIndexed] = React.useState(true);
  const [newFolder, setNewFolder] = React.useState("");

  const listing = useQuery({
    queryKey: ["picker", current],
    queryFn: () => api.listDirectory(current!, "name", "asc", 1, 0),
    enabled: current !== null && indexed,
  });

  const nameError = newFolder.trim() ? invalidSubpathReason(newFolder) : null;
  const invalidName = nameError !== null;
  const target = current ? joinSubpath(current, newFolder) : null;

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={title}
        description="Only folders inside locations you have authorized are available."
        className="w-[min(640px,94vw)]"
        footer={
          <>
            <div
              className="selectable mr-auto min-w-0 truncate text-[12px] text-muted"
              title={target ?? ""}
            >
              {target ?? "Choose a folder"}
            </div>
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={!target || invalidName}
              onClick={() => target && onPick(target)}
            >
              {confirmLabel}
            </Button>
          </>
        }
      >
        <div className="grid h-[380px] grid-cols-[200px_1fr]">
          <div className="overflow-auto border-r border-border p-2">
            <div className="px-2 pb-1.5 text-[11px] font-medium tracking-wide text-subtle uppercase">
              Locations
            </div>
            {locations.data?.map((l) => (
              <button
                key={l.id}
                onClick={() => {
                  setCurrent(l.path);
                  setIndexed(l.kind === "scan");
                  setNewFolder("");
                }}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12.5px] hover:bg-surface-2",
                  current?.startsWith(l.path) && "bg-surface-2 text-fg",
                )}
                title={l.path}
              >
                <HardDrive className="size-3.5 shrink-0 text-muted" />
                <span className="truncate">
                  {l.path.split(/[\\/]/).filter(Boolean).pop() ?? l.path}
                </span>
              </button>
            ))}
          </div>
          <div className="flex min-h-0 flex-col">
            {current === null ? (
              <div className="p-6 text-[13px] text-muted">
                Select a location to browse its folders.
              </div>
            ) : (
              <>
                {indexed && listing.data && (
                  <div className="flex flex-wrap items-center gap-0.5 border-b border-border px-3 py-2 text-[12.5px]">
                    {listing.data.breadcrumbs.map((b, i) => (
                      <React.Fragment key={b.path}>
                        {i > 0 && <ChevronRight className="size-3.5 text-subtle" />}
                        <button
                          className="rounded px-1 hover:bg-surface-2"
                          onClick={() => setCurrent(b.path)}
                        >
                          {b.name}
                        </button>
                      </React.Fragment>
                    ))}
                  </div>
                )}
                <div className="min-h-0 flex-1 overflow-auto p-1.5">
                  {!indexed ? (
                    <div className="p-4 text-[13px] text-muted">
                      This destination isn’t indexed, so its subfolders can’t be browsed. Files will
                      go directly into it, or into a new subfolder you name below.
                    </div>
                  ) : listing.isLoading ? (
                    <LoadingState />
                  ) : listing.isError ? (
                    <ErrorState error={listing.error} onRetry={() => listing.refetch()} />
                  ) : listing.data?.subdirectories.length === 0 ? (
                    <div className="p-4 text-[13px] text-muted">No subfolders.</div>
                  ) : (
                    listing.data?.subdirectories.map((d) => (
                      <button
                        key={d.path}
                        onDoubleClick={() => setCurrent(d.path)}
                        onClick={() => setCurrent(d.path)}
                        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-surface-2"
                      >
                        <Folder className="size-4 shrink-0 text-accent" strokeWidth={1.75} />
                        <span className="truncate">{d.name}</span>
                        <ChevronRight className="ml-auto size-3.5 text-subtle" />
                      </button>
                    ))
                  )}
                </div>
                <div className="flex items-center gap-2 border-t border-border p-3">
                  <FolderPlus className="size-4 shrink-0 text-muted" />
                  <Input
                    value={newFolder}
                    onChange={(e) => setNewFolder(e.target.value)}
                    placeholder="Optional new subfolder, e.g. Reports/2024"
                    aria-label="New subfolder"
                    aria-invalid={invalidName}
                  />
                </div>
                {invalidName && (
                  <div className="px-3 pb-2 text-[12px] text-danger">{nameError}</div>
                )}
              </>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
