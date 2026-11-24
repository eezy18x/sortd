import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { FileBox, FolderInput, Trash2, X } from "lucide-react";
import { api } from "@/services/api";
import type { Category, FileFilter } from "@/types";
import { CATEGORIES } from "@/lib/categories";
import { DAY, formatBytes, nowSeconds } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/primitives";
import { EmptyState, Page, PageHeader } from "@/components/common";
import { FileTable, useInfiniteFiles, type SortState } from "@/components/files/FileTable";
import { useFileActions } from "@/components/files/FileActions";

const SIZES = [10, 50, 100, 500, 1024, 4096].map((mb) => mb * 1024 * 1024);
const AGES: { label: string; before?: number; after?: number }[] = [
  { label: "Any time" },
  { label: "Not modified in 6 months", before: 182 },
  { label: "Not modified in 1 year", before: 365 },
  { label: "Not modified in 3 years", before: 3 * 365 },
  { label: "Modified in last 30 days", after: 30 },
];

export function LargeFilesPage() {
  const actions = useFileActions();
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });
  const [minSize, setMinSize] = React.useState(SIZES[2]!);
  const [category, setCategory] = React.useState<Category | "">("");
  const [locationId, setLocationId] = React.useState<number | "">("");
  const [age, setAge] = React.useState(0);
  const [sort, setSort] = React.useState<SortState>({ field: "size", order: "desc" });
  const [selection, setSelection] = React.useState<Set<number>>(new Set());

  const filter: FileFilter = React.useMemo(() => {
    const a = AGES[age]!;
    const now = nowSeconds();
    return {
      minSize,
      categories: category ? [category] : [],
      locationId: locationId === "" ? undefined : locationId,
      modifiedBefore: a.before ? now - a.before * DAY : undefined,
      modifiedAfter: a.after ? now - a.after * DAY : undefined,
    };
  }, [minSize, category, locationId, age]);

  const { query, files, total } = useInfiniteFiles(["large", filter, sort], (offset, limit) =>
    api.queryFiles(filter, sort.field, sort.order, limit, offset),
  );
  const selected = files.filter((f) => selection.has(f.id));
  const selectedBytes = selected.reduce((s, f) => s + f.size, 0);

  return (
    <Page className="h-full">
      <PageHeader
        title="Large Files"
        description="The biggest files in your indexed locations. Review them before deciding what to keep."
      />
      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={minSize}
          onChange={(e) => setMinSize(Number(e.target.value))}
          aria-label="Minimum size"
        >
          {SIZES.map((s) => (
            <option key={s} value={s}>
              Larger than {formatBytes(s, 0)}
            </option>
          ))}
        </Select>
        <Select
          value={category}
          onChange={(e) => setCategory(e.target.value as Category | "")}
          aria-label="Category"
        >
          <option value="">All categories</option>
          {CATEGORIES.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </Select>
        <Select
          value={age}
          onChange={(e) => setAge(Number(e.target.value))}
          aria-label="Modification date"
        >
          {AGES.map((a, i) => (
            <option key={a.label} value={i}>
              {a.label}
            </option>
          ))}
        </Select>
        <Select
          value={locationId}
          onChange={(e) => setLocationId(e.target.value ? Number(e.target.value) : "")}
          aria-label="Location"
        >
          <option value="">All locations</option>
          {locations.data
            ?.filter((l) => l.kind === "scan")
            .map((l) => (
              <option key={l.id} value={l.id}>
                {l.path}
              </option>
            ))}
        </Select>
        {selection.size > 0 && (
          <div className="ml-auto flex items-center gap-1.5">
            <span className="tabular mr-1 text-[12.5px] text-muted">
              {selection.size} selected · {formatBytes(selectedBytes)}
            </span>
            <Button size="sm" onClick={() => actions.moveTo(selected)}>
              <FolderInput /> Move
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
        )}
      </div>
      <FileTable
        className="min-h-[360px] flex-1"
        files={files}
        total={total}
        isLoading={query.isLoading}
        error={query.error}
        onRetry={() => query.refetch()}
        hasMore={query.hasNextPage}
        onLoadMore={() => !query.isFetchingNextPage && void query.fetchNextPage()}
        sort={sort}
        onSort={setSort}
        selection={selection}
        onSelectionChange={setSelection}
        empty={
          <EmptyState
            icon={FileBox}
            title="No files match"
            description="Try a smaller size threshold or a different filter."
          />
        }
      />
    </Page>
  );
}
