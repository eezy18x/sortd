import * as React from "react";
import { useSearchParams } from "react-router-dom";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { History, Search, X } from "lucide-react";
import { api } from "@/services/api";
import type { Category, SortField } from "@/types";
import { CATEGORIES } from "@/lib/categories";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Card, Kbd, Select } from "@/components/ui/primitives";
import { EmptyState, Page, PageHeader } from "@/components/common";
import { FileTable, type SortState } from "@/components/files/FileTable";
import { useDebouncedValue } from "@/hooks/useTheme";

const SYNTAX: [string, string][] = [
  ["ext:pdf,docx", "by extension"],
  ["cat:images", "by category"],
  ["size>100mb", "by size (>, >=, <, <=)"],
  ["newer:7d", "modified within (d, w, m, y)"],
  ["older:1y", "not modified within"],
  ["in:path", "match folder names too"],
  ['"exact phrase"', "keep words together"],
];

export function SearchPage() {
  const qc = useQueryClient();
  const [params, setParams] = useSearchParams();
  const q = params.get("q") ?? "";
  const [input, setInput] = React.useState(q);
  const [category, setCategory] = React.useState<Category | "">("");
  const [sort, setSort] = React.useState<SortState | null>(null);
  const [showSuggest, setShowSuggest] = React.useState(false);
  const [active, setActive] = React.useState(-1);
  const debounced = useDebouncedValue(input, 180);

  // Sync the local input with the URL when navigating from the header box.
  const [prevQ, setPrevQ] = React.useState(q);
  if (q !== prevQ) {
    setPrevQ(q);
    setInput(q);
  }

  const history = useQuery({ queryKey: ["search-history"], queryFn: api.searchHistory });
  const suggestions = useQuery({
    queryKey: ["suggest", debounced],
    queryFn: () => api.suggestions(debounced),
    enabled: showSuggest && debounced.trim().length > 0,
  });
  const clearHistory = useMutation({
    mutationFn: api.clearSearchHistory,
    onSuccess: () => qc.invalidateQueries({ queryKey: ["search-history"] }),
  });

  const filter = React.useMemo(() => ({ categories: category ? [category] : [] }), [category]);
  const results = useInfiniteQuery({
    queryKey: ["search", q, filter, sort],
    enabled: q.length > 0 || !!category,
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      api.search({
        query: q,
        filter,
        sort: sort?.field,
        order: sort?.order,
        limit: 200,
        offset: pageParam,
      }),
    getNextPageParam: (last) => {
      const p = last.page;
      return p.offset + p.items.length < p.total ? p.offset + p.items.length : undefined;
    },
  });
  const files = React.useMemo(
    () => results.data?.pages.flatMap((p) => p.page.items) ?? [],
    [results.data],
  );
  const total = results.data?.pages[0]?.page.total ?? 0;
  const elapsed = results.data?.pages[0]?.elapsedMs;

  const submit = (value: string) => {
    const v = value.trim();
    setShowSuggest(false);
    setActive(-1);
    setParams(v ? { q: v } : {});
    if (v)
      void api.recordSearch(v).then(() => qc.invalidateQueries({ queryKey: ["search-history"] }));
  };

  const items = suggestions.data ?? [];

  return (
    <Page className="h-full">
      <PageHeader
        title="Search"
        description="Instant search across names and paths of everything SORTD has indexed. File contents are not indexed."
      />
      <div className="relative">
        <form
          role="search"
          onSubmit={(e) => {
            e.preventDefault();
            submit(active >= 0 && items[active] ? items[active].text : input);
          }}
        >
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-subtle" />
          <input
            autoFocus
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              setShowSuggest(true);
              setActive(-1);
            }}
            onBlur={() => window.setTimeout(() => setShowSuggest(false), 120)}
            onKeyDown={(e) => {
              if (!showSuggest || items.length === 0) return;
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setActive((a) => Math.min(items.length - 1, a + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setActive((a) => Math.max(-1, a - 1));
              } else if (e.key === "Escape") {
                setShowSuggest(false);
              }
            }}
            placeholder="Search by name, or try ext:pdf size>10mb"
            aria-label="Search query"
            aria-autocomplete="list"
            aria-controls="search-suggestions"
            aria-activedescendant={active >= 0 ? `suggestion-${active}` : undefined}
            className="h-10 w-full rounded-lg border border-border bg-surface pr-10 pl-9 text-[14px] placeholder:text-subtle hover:border-border-strong focus-visible:border-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/25"
          />
          {input && (
            <button
              type="button"
              aria-label="Clear search"
              className="absolute top-1/2 right-2.5 -translate-y-1/2 rounded p-1 text-subtle hover:text-fg"
              onClick={() => {
                setInput("");
                submit("");
              }}
            >
              <X className="size-4" />
            </button>
          )}
        </form>
        {showSuggest && items.length > 0 && (
          <ul
            id="search-suggestions"
            role="listbox"
            className="absolute inset-x-0 top-11 z-30 animate-fade-in overflow-hidden rounded-lg border border-border bg-surface py-1 shadow-elevated"
          >
            {items.map((s, i) => (
              <li
                key={`${s.source}-${s.text}`}
                id={`suggestion-${i}`}
                role="option"
                aria-selected={i === active}
                onMouseDown={(e) => {
                  e.preventDefault();
                  setInput(s.text);
                  submit(s.text);
                }}
                className={cn(
                  "flex cursor-default items-center gap-2.5 px-3 py-1.5 text-[13px]",
                  i === active && "bg-surface-2",
                )}
              >
                {s.source === "history" ? (
                  <History className="size-3.5 text-subtle" />
                ) : (
                  <Search className="size-3.5 text-subtle" />
                )}
                <span className="truncate">{s.text}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2">
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
          value={sort ? `${sort.field}:${sort.order}` : ""}
          onChange={(e) => {
            const [field, order] = e.target.value.split(":");
            setSort(
              e.target.value ? { field: field as SortField, order: order as "asc" | "desc" } : null,
            );
          }}
          aria-label="Sort"
        >
          <option value="">Best match</option>
          <option value="name:asc">Name</option>
          <option value="size:desc">Largest first</option>
          <option value="modified:desc">Newest first</option>
          <option value="modified:asc">Oldest first</option>
        </Select>
        {(q || category) && results.data && (
          <span className="tabular text-[12.5px] text-muted">
            {total.toLocaleString()} result{total === 1 ? "" : "s"}
            {elapsed !== undefined && ` · ${elapsed} ms`}
          </span>
        )}
      </div>

      {!q && !category ? (
        <div className="grid gap-4 lg:grid-cols-2">
          <Card className="px-5 py-4">
            <div className="mb-3 flex items-center justify-between">
              <h3 className="text-[13.5px] font-semibold">Recent searches</h3>
              {!!history.data?.length && (
                <Button variant="ghost" size="sm" onClick={() => clearHistory.mutate()}>
                  Clear
                </Button>
              )}
            </div>
            {history.data?.length ? (
              <div className="flex flex-wrap gap-1.5">
                {history.data.map((h) => (
                  <button
                    key={h}
                    className="rounded-md border border-border bg-surface-2 px-2 py-1 text-[12.5px] hover:border-border-strong"
                    onClick={() => submit(h)}
                  >
                    {h}
                  </button>
                ))}
              </div>
            ) : (
              <p className="text-[12.5px] text-muted">
                No saved searches. History is stored only on this computer and can be turned off in
                Settings.
              </p>
            )}
          </Card>
          <Card className="px-5 py-4">
            <h3 className="mb-3 text-[13.5px] font-semibold">Search syntax</h3>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
              {SYNTAX.map(([k, v]) => (
                <React.Fragment key={k}>
                  <dt>
                    <code className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[12px]">
                      {k}
                    </code>
                  </dt>
                  <dd className="text-muted">{v}</dd>
                </React.Fragment>
              ))}
            </dl>
            <p className="mt-3 flex items-center gap-1.5 text-[12px] text-subtle">
              Press <Kbd>Ctrl</Kbd>
              <Kbd>K</Kbd> anywhere to jump to search.
            </p>
          </Card>
        </div>
      ) : (
        <FileTable
          className="min-h-[320px] flex-1"
          files={files}
          total={total}
          isLoading={results.isLoading}
          error={results.error}
          onRetry={() => results.refetch()}
          hasMore={results.hasNextPage}
          onLoadMore={() => !results.isFetchingNextPage && void results.fetchNextPage()}
          sort={sort ?? undefined}
          onSort={setSort}
          empty={
            <EmptyState
              icon={Search}
              title="No matches"
              description="Check the spelling, remove a filter, or add in:path to match folder names."
            />
          }
        />
      )}
    </Page>
  );
}
