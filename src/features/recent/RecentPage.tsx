import * as React from "react";
import { Clock } from "lucide-react";
import { api } from "@/services/api";
import type { Category, FileFilter } from "@/types";
import { CATEGORIES } from "@/lib/categories";
import { DAY, nowSeconds } from "@/lib/format";
import { Select } from "@/components/ui/primitives";
import { EmptyState, Page, PageHeader } from "@/components/common";
import { FileTable, useInfiniteFiles, type SortState } from "@/components/files/FileTable";

const RANGES = [
  { label: "Last 24 hours", days: 1 },
  { label: "Last 7 days", days: 7 },
  { label: "Last 30 days", days: 30 },
  { label: "Last 90 days", days: 90 },
];

export function RecentPage() {
  const [range, setRange] = React.useState(1);
  const [category, setCategory] = React.useState<Category | "">("");
  const [field, setField] = React.useState<"modified" | "created">("modified");
  const [sort, setSort] = React.useState<SortState>({ field: "modified", order: "desc" });

  const filter: FileFilter = React.useMemo(() => {
    const since = nowSeconds() - RANGES[range]!.days * DAY;
    return {
      categories: category ? [category] : [],
      ...(field === "modified" ? { modifiedAfter: since } : { createdAfter: since }),
    };
  }, [range, category, field]);

  const { query, files, total } = useInfiniteFiles(["recent", filter, sort], (offset, limit) =>
    api.queryFiles(filter, sort.field, sort.order, limit, offset),
  );

  return (
    <Page className="h-full">
      <PageHeader
        title="Recent Files"
        description="Files changed or created recently, as of the last scan. Creation times are shown where the filesystem reports them."
      />
      <div className="flex flex-wrap gap-2">
        <Select
          value={field}
          onChange={(e) => {
            const f = e.target.value as "modified" | "created";
            setField(f);
            setSort({ field: f, order: "desc" });
          }}
          aria-label="Date field"
        >
          <option value="modified">Modified</option>
          <option value="created">Created</option>
        </Select>
        <Select
          value={range}
          onChange={(e) => setRange(Number(e.target.value))}
          aria-label="Time range"
        >
          {RANGES.map((r, i) => (
            <option key={r.label} value={i}>
              {r.label}
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
        dateField={field}
        empty={
          <EmptyState
            icon={Clock}
            title="Nothing in this period"
            description="Try a longer time range, or rescan to pick up recent changes."
          />
        }
      />
    </Page>
  );
}
