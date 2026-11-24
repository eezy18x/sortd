import * as React from "react";
import { useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { Shapes } from "lucide-react";
import { api } from "@/services/api";
import type { Category } from "@/types";
import { CATEGORIES } from "@/lib/categories";
import { formatBytes, formatCount, formatPercent } from "@/lib/format";
import { cn } from "@/lib/utils";
import { EmptyState, ErrorState, Page, PageHeader } from "@/components/common";
import { Skeleton } from "@/components/ui/primitives";
import { FileTable, useInfiniteFiles, type SortState } from "@/components/files/FileTable";

export function CategoriesPage() {
  const [params, setParams] = useSearchParams();
  const active = (params.get("c") as Category | null) ?? null;
  const [sort, setSort] = React.useState<SortState>({ field: "size", order: "desc" });
  const stats = useQuery({ queryKey: ["category-stats"], queryFn: () => api.categoryStats() });
  const byId = new Map(stats.data?.map((s) => [s.category, s]));
  const totalSize = stats.data?.reduce((s, c) => s + c.size, 0) ?? 0;

  const { query, files, total } = useInfiniteFiles(
    ["category-files", active, sort],
    (offset, limit) =>
      api.queryFiles({ categories: [active!] }, sort.field, sort.order, limit, offset),
    !!active,
  );

  return (
    <Page className={active ? "h-full" : undefined}>
      <PageHeader
        title="Categories"
        description="Files grouped by type. Categories come from extensions and names (treated as hints) and can be overridden per file from its details panel."
      />
      {stats.isError ? (
        <ErrorState error={stats.error} onRetry={() => stats.refetch()} />
      ) : (
        <div className="grid grid-cols-2 gap-2.5 md:grid-cols-4 xl:grid-cols-7">
          {stats.isLoading
            ? Array.from({ length: 7 }, (_, i) => (
                <Skeleton key={i} className="h-[88px] rounded-xl" />
              ))
            : CATEGORIES.map((c) => {
                const s = byId.get(c.id);
                const selected = active === c.id;
                return (
                  <button
                    key={c.id}
                    onClick={() => setParams(selected ? {} : { c: c.id })}
                    aria-pressed={selected}
                    title={c.description}
                    className={cn(
                      "flex flex-col items-start rounded-xl border border-border bg-surface px-3.5 py-3 text-left transition-colors hover:border-border-strong hover:bg-surface-2",
                      selected &&
                        "border-accent bg-accent-soft hover:border-accent hover:bg-accent-soft",
                      !s && "opacity-55",
                    )}
                  >
                    <c.icon
                      className={cn("size-4", selected ? "text-accent" : "text-muted")}
                      strokeWidth={1.75}
                    />
                    <span className="mt-2 text-[13px] font-medium">{c.label}</span>
                    <span className="tabular text-[11.5px] text-muted">
                      {s ? `${formatCount(s.files)} · ${formatBytes(s.size)}` : "None"}
                    </span>
                    {s && totalSize > 0 && (
                      <span className="tabular text-[11px] text-subtle">
                        {formatPercent(s.size / totalSize, 1)} of indexed
                      </span>
                    )}
                  </button>
                );
              })}
        </div>
      )}
      {active ? (
        <FileTable
          className="min-h-[320px] flex-1"
          files={files}
          total={total}
          isLoading={query.isLoading}
          error={query.error}
          onRetry={() => query.refetch()}
          hasMore={query.hasNextPage}
          onLoadMore={() => !query.isFetchingNextPage && void query.fetchNextPage()}
          sort={sort}
          onSort={setSort}
          empty={<EmptyState icon={Shapes} title="No files in this category" />}
        />
      ) : (
        <div className="rounded-xl border border-dashed border-border px-6 py-10 text-center text-[13px] text-muted">
          Select a category to list its files.
        </div>
      )}
    </Page>
  );
}
