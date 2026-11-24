import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowRight,
  ChevronDown,
  CircleSlash,
  FolderCog,
  Info,
  Sparkles,
  Undo2,
} from "lucide-react";
import { api, errorMessage } from "@/services/api";
import type { Recommendation } from "@/types";
import { categoryMeta } from "@/lib/categories";
import { formatBytes, formatPercent, relativeTo } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Badge, Card, Checkbox, Select } from "@/components/ui/primitives";
import {
  EmptyState,
  ErrorState,
  InlineNotice,
  LoadingState,
  Page,
  PageHeader,
} from "@/components/common";
import { useFileActions } from "@/components/files/FileActions";
import { useToast } from "@/components/toast";

function confidenceTone(c: number) {
  return c >= 0.8 ? "success" : c >= 0.6 ? "accent" : "warning";
}

function RecommendationRow({
  r,
  selected,
  onSelect,
  base,
  status,
}: {
  r: Recommendation;
  selected: boolean;
  onSelect: (on: boolean) => void;
  base: string | null;
  status: string;
}) {
  const [open, setOpen] = React.useState(false);
  const actions = useFileActions();
  const Icon = categoryMeta(r.file.category).icon;
  return (
    <li className={cn("border-b border-border/60 last:border-0", selected && "bg-accent-soft/40")}>
      <div className="flex items-start gap-3 px-4 py-3">
        {status === "pending" && (
          <Checkbox
            className="mt-0.5"
            checked={selected}
            onCheckedChange={onSelect}
            aria-label={`Select ${r.file.name}`}
          />
        )}
        <Icon className="mt-0.5 size-4 shrink-0 text-muted" strokeWidth={1.75} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px]">
            <button
              className="truncate font-medium hover:underline"
              onClick={() => actions.details(r.file)}
            >
              {r.file.name}
            </button>
            <span className="text-[11.5px] text-subtle">{formatBytes(r.file.size)}</span>
          </div>
          <div className="mt-1 flex min-w-0 items-center gap-1.5 text-[12px]">
            <span className="selectable truncate text-subtle" title={r.file.parent}>
              {r.file.parent}
            </span>
            <ArrowRight className="size-3.5 shrink-0 text-accent" />
            <span className="selectable truncate font-medium text-fg" title={r.destination}>
              {base ? relativeTo(r.destination, base) : r.destination}
            </span>
          </div>
          <div className="mt-1 text-[12.5px] text-muted">{r.reason}</div>
          {open && (
            <ul className="mt-2 space-y-1 rounded-lg bg-surface-2 px-3 py-2 text-[12px] text-muted">
              {r.signals.map((s, i) => (
                <li key={i} className="flex gap-2">
                  <span className="mt-[7px] size-1 shrink-0 rounded-full bg-subtle" />
                  {s}
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          <Badge tone={confidenceTone(r.confidence)}>{formatPercent(r.confidence)}</Badge>
          <button
            className="flex items-center gap-0.5 text-[11.5px] text-muted hover:text-fg"
            onClick={() => setOpen((o) => !o)}
            aria-expanded={open}
          >
            Why <ChevronDown className={cn("size-3 transition-transform", open && "rotate-180")} />
          </button>
        </div>
      </div>
    </li>
  );
}

export function OrganizationPage() {
  const qc = useQueryClient();
  const toast = useToast();
  const actions = useFileActions();
  const [status, setStatus] = React.useState<"pending" | "dismissed" | "applied">("pending");
  const [minConfidence, setMinConfidence] = React.useState(0);
  const [selection, setSelection] = React.useState<Set<number>>(new Set());
  const settings = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const recs = useQuery({
    queryKey: ["recommendations", status, minConfidence],
    queryFn: () => api.recommendations(status, minConfidence, 500, 0),
  });

  const generate = useMutation({
    mutationFn: api.generateRecommendations,
    onSuccess: (r) => {
      void qc.invalidateQueries();
      setSelection(new Set());
      toast.success(
        "Suggestions updated",
        `${r.generated} suggestion${r.generated === 1 ? "" : "s"} from ${r.examined} files in catch-all folders.`,
      );
    },
    onError: (e) => toast.error("Couldn’t generate suggestions", errorMessage(e)),
  });
  const setRecStatus = useMutation({
    mutationFn: ({ ids, to }: { ids: number[]; to: "pending" | "dismissed" }) =>
      api.setRecommendationStatus(ids, to),
    onSuccess: () => {
      setSelection(new Set());
      void qc.invalidateQueries();
    },
    onError: (e) => toast.error("Couldn’t update suggestions", errorMessage(e)),
  });
  const pickRoot = useMutation({
    mutationFn: api.pickOrganizationRoot,
    onSuccess: () => void qc.invalidateQueries(),
    onError: (e) => toast.error("Couldn’t set the folder", errorMessage(e)),
  });

  const items = recs.data?.items ?? [];
  const selected = items.filter((r) => selection.has(r.id));
  const all = items.length > 0 && selected.length === items.length;

  const apply = () => {
    if (!selected.length) return;
    actions.preview(
      {
        label: `Organize ${selected.length} file${selected.length === 1 ? "" : "s"}`,
        origin: "organization",
        operations: selected.map((r) => ({
          op: "move",
          source: r.file.path,
          destinationDir: r.destination,
        })),
      },
      () => setSelection(new Set()),
    );
  };

  const base = settings.data?.organizationRoot ?? null;

  return (
    <Page>
      <PageHeader
        title="Smart Organization"
        description="Suggestions for files sitting in catch-all folders like Downloads and Desktop. Based on file type, name keywords and your existing folders — computed locally. Nothing moves until you review and approve it."
        actions={
          <Button variant="primary" onClick={() => generate.mutate()} loading={generate.isPending}>
            <Sparkles />{" "}
            {recs.data && recs.data.total > 0 ? "Refresh suggestions" : "Generate suggestions"}
          </Button>
        }
      />

      <Card className="flex flex-wrap items-center gap-3 px-4 py-3 text-[12.5px]">
        <FolderCog className="size-4 text-muted" />
        <span className="text-muted">Organize into:</span>
        <span className="selectable font-medium">
          {base ?? "The root of each indexed location"}
        </span>
        <div className="ml-auto flex gap-1.5">
          <Button size="sm" onClick={() => pickRoot.mutate()} loading={pickRoot.isPending}>
            Choose folder…
          </Button>
          {base && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => api.clearOrganizationRoot().then(() => qc.invalidateQueries())}
            >
              Reset
            </Button>
          )}
        </div>
      </Card>

      <div className="flex flex-wrap items-center gap-2">
        <div
          className="flex rounded-lg border border-border bg-surface p-0.5"
          role="tablist"
          aria-label="Suggestion status"
        >
          {(["pending", "dismissed", "applied"] as const).map((s) => (
            <button
              key={s}
              role="tab"
              aria-selected={status === s}
              onClick={() => {
                setStatus(s);
                setSelection(new Set());
              }}
              className={cn(
                "h-7 rounded-md px-3 text-[12.5px] capitalize text-muted",
                status === s && "bg-surface-2 text-fg",
              )}
            >
              {s === "pending" ? "To review" : s}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-2 text-[12.5px] text-muted">
          Minimum confidence
          <Select
            value={minConfidence}
            onChange={(e) => setMinConfidence(Number(e.target.value))}
            className="h-7"
          >
            <option value={0}>Any</option>
            <option value={0.6}>60%</option>
            <option value={0.75}>75%</option>
            <option value={0.85}>85%</option>
          </Select>
        </label>
        {status === "pending" && items.length > 0 && (
          <div className="ml-auto flex items-center gap-2">
            <label className="flex items-center gap-2 text-[12.5px] text-muted">
              <Checkbox
                checked={all ? true : selected.length ? "indeterminate" : false}
                onCheckedChange={(on) =>
                  setSelection(on ? new Set(items.map((r) => r.id)) : new Set())
                }
                aria-label="Select all suggestions"
              />
              Select all
            </label>
            <Button
              size="sm"
              variant="ghost"
              disabled={!selected.length}
              onClick={() =>
                setRecStatus.mutate({ ids: selected.map((r) => r.id), to: "dismissed" })
              }
            >
              <CircleSlash /> Dismiss
            </Button>
            <Button size="sm" variant="primary" disabled={!selected.length} onClick={apply}>
              Review {selected.length || ""} move{selected.length === 1 ? "" : "s"}
            </Button>
          </div>
        )}
        {status === "dismissed" && items.length > 0 && (
          <Button
            size="sm"
            className="ml-auto"
            disabled={!items.length}
            onClick={() => setRecStatus.mutate({ ids: items.map((r) => r.id), to: "pending" })}
          >
            <Undo2 /> Restore all
          </Button>
        )}
      </div>

      <Card>
        {recs.isLoading ? (
          <LoadingState />
        ) : recs.isError ? (
          <ErrorState error={recs.error} onRetry={() => recs.refetch()} />
        ) : items.length === 0 ? (
          <EmptyState
            icon={Sparkles}
            title={
              status === "pending"
                ? "No suggestions to review"
                : status === "dismissed"
                  ? "Nothing dismissed"
                  : "Nothing applied yet"
            }
            description={
              status === "pending"
                ? "Generate suggestions after scanning. SORTD looks at files directly in Downloads, Desktop and similar folders, and at loose files in the root of each location."
                : undefined
            }
          />
        ) : (
          <ul>
            {items.map((r) => (
              <RecommendationRow
                key={r.id}
                r={r}
                base={base}
                status={status}
                selected={selection.has(r.id)}
                onSelect={(on) => {
                  const next = new Set(selection);
                  if (on) next.add(r.id);
                  else next.delete(r.id);
                  setSelection(next);
                }}
              />
            ))}
          </ul>
        )}
      </Card>

      <InlineNotice icon={Info}>
        Confidence reflects how strongly the signals agree: category certainty, topic and
        document-type keywords in the name, and whether a matching folder already exists.
        Suggestions never use a remote AI service.
      </InlineNotice>
    </Page>
  );
}
