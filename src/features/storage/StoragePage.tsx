import * as React from "react";
import { useNavigate } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip as ChartTooltip,
  XAxis,
  YAxis,
} from "recharts";
import { CircleAlert, Folder, HardDrive, Table2 } from "lucide-react";
import { api } from "@/services/api";
import { categoryMeta } from "@/lib/categories";
import { formatBytes, formatCount, formatPercent } from "@/lib/format";
import { Button } from "@/components/ui/button";
import { Card, CardHeader, Select, Skeleton, Tooltip } from "@/components/ui/primitives";
import { ErrorState, InlineNotice, Page, PageHeader, SizeBar, StatTile } from "@/components/common";
import { useFileActions } from "@/components/files/FileActions";

interface Datum {
  label: string;
  size: number;
  files: number;
}

function ChartTip({ active, payload }: { active?: boolean; payload?: { payload: Datum }[] }) {
  const d = payload?.[0]?.payload;
  if (!active || !d) return null;
  return (
    <div className="rounded-md border border-border bg-surface-3 px-2.5 py-1.5 text-[12px] shadow-elevated">
      <div className="font-medium text-fg">{d.label}</div>
      <div className="tabular text-muted">
        {formatBytes(d.size)} · {formatCount(d.files)} files
      </div>
    </div>
  );
}

const axisTick = { fill: "var(--fg-muted)", fontSize: 11.5 };

/** Horizontal magnitude bars: one hue, value carried by length + tooltip + table. */
function HorizontalBars({ data, height }: { data: Datum[]; height: number }) {
  return (
    <ResponsiveContainer width="100%" height={height}>
      <BarChart
        data={data}
        layout="vertical"
        margin={{ top: 4, right: 16, bottom: 4, left: 4 }}
        barCategoryGap={6}
      >
        <CartesianGrid horizontal={false} stroke="var(--border)" />
        <XAxis
          type="number"
          tickFormatter={(v: number) => formatBytes(v, 0)}
          tick={axisTick}
          axisLine={false}
          tickLine={false}
        />
        <YAxis
          type="category"
          dataKey="label"
          width={104}
          tick={axisTick}
          axisLine={false}
          tickLine={false}
        />
        <ChartTooltip content={<ChartTip />} cursor={{ fill: "var(--surface-2)" }} />
        <Bar
          dataKey="size"
          fill="var(--chart)"
          radius={[0, 4, 4, 0]}
          maxBarSize={18}
          isAnimationActive={false}
        />
      </BarChart>
    </ResponsiveContainer>
  );
}

function ColumnBars({ data, height }: { data: Datum[]; height: number }) {
  return (
    <ResponsiveContainer width="100%" height={height}>
      <BarChart data={data} margin={{ top: 8, right: 8, bottom: 4, left: 4 }} barCategoryGap="22%">
        <CartesianGrid vertical={false} stroke="var(--border)" />
        <XAxis dataKey="label" tick={axisTick} axisLine={false} tickLine={false} interval={0} />
        <YAxis
          tickFormatter={(v: number) => formatBytes(v, 0)}
          tick={axisTick}
          axisLine={false}
          tickLine={false}
          width={64}
        />
        <ChartTooltip content={<ChartTip />} cursor={{ fill: "var(--surface-2)" }} />
        <Bar
          dataKey="size"
          fill="var(--chart)"
          radius={[4, 4, 0, 0]}
          maxBarSize={44}
          isAnimationActive={false}
        />
      </BarChart>
    </ResponsiveContainer>
  );
}

function DataTable({ rows }: { rows: Datum[] }) {
  return (
    <table className="w-full text-[12.5px]">
      <thead className="text-left text-[11.5px] text-muted">
        <tr className="border-b border-border">
          <th className="py-1.5 font-medium">Group</th>
          <th className="py-1.5 text-right font-medium">Files</th>
          <th className="py-1.5 text-right font-medium">Size</th>
        </tr>
      </thead>
      <tbody className="tabular">
        {rows.map((r) => (
          <tr key={r.label} className="border-b border-border/60 last:border-0">
            <td className="py-1.5">{r.label}</td>
            <td className="py-1.5 text-right">{formatCount(r.files)}</td>
            <td className="py-1.5 text-right">{formatBytes(r.size)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function ChartCard({
  title,
  description,
  data,
  kind,
}: {
  title: string;
  description: string;
  data: Datum[];
  kind: "h" | "v";
}) {
  const [table, setTable] = React.useState(false);
  return (
    <Card>
      <CardHeader
        title={title}
        description={description}
        actions={
          <Button
            variant="ghost"
            size="sm"
            aria-pressed={table}
            onClick={() => setTable((t) => !t)}
          >
            <Table2 /> {table ? "Chart" : "Table"}
          </Button>
        }
      />
      <div className="px-4 pb-4">
        {data.length === 0 ? (
          <div className="py-10 text-center text-[13px] text-muted">No data yet.</div>
        ) : table ? (
          <DataTable rows={data} />
        ) : kind === "h" ? (
          <HorizontalBars data={data} height={Math.max(160, data.length * 26 + 30)} />
        ) : (
          <ColumnBars data={data} height={240} />
        )}
      </div>
    </Card>
  );
}

export function StoragePage() {
  const navigate = useNavigate();
  const actions = useFileActions();
  const [locationId, setLocationId] = React.useState<number | undefined>(undefined);
  const locations = useQuery({ queryKey: ["locations"], queryFn: api.locations });
  const q = useQuery({ queryKey: ["storage", locationId], queryFn: () => api.storage(locationId) });
  const a = q.data;

  const categoryData: Datum[] = (a?.categories ?? []).map((c) => ({
    label: categoryMeta(c.category).label,
    size: c.size,
    files: c.files,
  }));
  const ageData: Datum[] = (a?.ageBuckets ?? []).map((b) => ({
    label: b.label,
    size: b.size,
    files: b.files,
  }));
  const maxDir = Math.max(1, ...(a?.largestDirectories.map((d) => d.totalSize) ?? [1]));

  return (
    <Page>
      <PageHeader
        title="Storage Analyzer"
        description="Where your indexed space goes. Indexed sizes count only the files SORTD has scanned; drive capacity is reported separately by the operating system."
        actions={
          <Select
            value={locationId ?? ""}
            onChange={(e) => setLocationId(e.target.value ? Number(e.target.value) : undefined)}
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
        }
      />

      {q.isError && <ErrorState error={q.error} onRetry={() => q.refetch()} />}

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {a ? (
          <>
            <StatTile label="Indexed size" value={formatBytes(a.indexedSize)} />
            <StatTile
              label="Files"
              value={formatCount(a.indexedFiles)}
              hint={`${formatCount(a.indexedDirectories)} folders`}
            />
            <StatTile
              label="Duplicate waste"
              value={formatBytes(a.duplicateWaste)}
              hint={`${formatCount(a.duplicateGroups)} groups (all locations)`}
              onClick={() => navigate("/duplicates")}
            />
            <StatTile
              label="Incomplete folders"
              value={formatCount(a.incompleteDirectories)}
              hint="Could not be fully read"
            />
          </>
        ) : (
          Array.from({ length: 4 }, (_, i) => <Skeleton key={i} className="h-[92px] rounded-xl" />)
        )}
      </div>

      {a && a.incompleteDirectories > 0 && (
        <InlineNotice tone="warning" icon={CircleAlert}>
          {formatCount(a.incompleteDirectories)} folders could not be fully read (permissions or
          errors). Sizes for them and their parents are shown as lower bounds (≥).
        </InlineNotice>
      )}

      {a && a.volumes.length > 0 && (
        <Card>
          <CardHeader
            title="Drives"
            description="Indexed data compared with the physical capacity of each location’s drive"
          />
          <ul className="flex flex-col gap-4 px-5 pb-5">
            {a.volumes.map((v) => {
              const used = v.disk ? v.disk.total - v.disk.available : 0;
              return (
                <li key={v.locationId}>
                  <div className="mb-1.5 flex flex-wrap items-center justify-between gap-2 text-[12.5px]">
                    <span className="selectable flex min-w-0 items-center gap-2">
                      <HardDrive className="size-4 shrink-0 text-muted" />
                      <span className="truncate">{v.path}</span>
                    </span>
                    <span className="tabular text-muted">
                      {v.complete ? "" : "≥ "}
                      {formatBytes(v.indexedSize)} indexed
                      {v.disk && (
                        <>
                          {" "}
                          · {formatBytes(used)} used of {formatBytes(v.disk.total)} ·{" "}
                          {formatBytes(v.disk.available)} free
                        </>
                      )}
                    </span>
                  </div>
                  {v.disk ? (
                    <Tooltip
                      content={`Indexed: ${formatPercent(v.indexedSize / v.disk.total, 1)} of drive · Used: ${formatPercent(used / v.disk.total, 1)}`}
                    >
                      <div
                        className="relative h-2.5 w-full overflow-hidden rounded-full bg-surface-3"
                        aria-label="Drive usage"
                      >
                        <div
                          className="absolute inset-y-0 left-0 rounded-full bg-[var(--chart-muted)]"
                          style={{ width: `${(used / v.disk.total) * 100}%` }}
                        />
                        <div
                          className="absolute inset-y-0 left-0 rounded-full bg-accent"
                          style={{
                            width: `${(Math.min(used, v.indexedSize) / v.disk.total) * 100}%`,
                          }}
                        />
                      </div>
                    </Tooltip>
                  ) : (
                    <div className="text-[12px] text-subtle">
                      Drive capacity unavailable for this location.
                    </div>
                  )}
                </li>
              );
            })}
            <li className="flex gap-4 text-[11.5px] text-muted">
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-accent" /> Indexed by SORTD
              </span>
              <span className="flex items-center gap-1.5">
                <span className="size-2.5 rounded-full bg-[var(--chart-muted)]" /> Other used space
              </span>
            </li>
          </ul>
        </Card>
      )}

      <div className="grid gap-4 lg:grid-cols-2">
        <ChartCard
          title="By category"
          description="Indexed size per category"
          data={categoryData}
          kind="h"
        />
        <ChartCard
          title="By age"
          description="Indexed size by last modification"
          data={ageData}
          kind="v"
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-[1.3fr_1fr]">
        <Card>
          <CardHeader title="Largest folders" description="Total size including subfolders" />
          <ul className="px-2 pb-2">
            {a?.largestDirectories.map((d) => (
              <li key={d.path}>
                <button
                  className="grid w-full grid-cols-[minmax(0,1fr)_120px_84px] items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface-2"
                  onClick={() => navigate(`/explorer?path=${encodeURIComponent(d.path)}`)}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <Folder className="size-4 shrink-0 text-accent" strokeWidth={1.75} />
                    <span className="min-w-0">
                      <span className="block truncate text-[13px]">{d.name}</span>
                      <span className="block truncate text-[11.5px] text-subtle">{d.parent}</span>
                    </span>
                  </span>
                  <SizeBar fraction={d.totalSize / maxDir} />
                  <span className="tabular text-right text-[12.5px]">
                    {!d.complete && "≥ "}
                    {formatBytes(d.totalSize)}
                  </span>
                </button>
              </li>
            ))}
            {a && a.largestDirectories.length === 0 && (
              <li className="px-3 py-6 text-[13px] text-muted">No folders indexed.</li>
            )}
          </ul>
        </Card>
        <Card>
          <CardHeader title="Top extensions" />
          <div className="px-5 pb-4">
            <table className="w-full text-[12.5px]">
              <tbody className="tabular">
                {a?.extensions.map((e) => (
                  <tr key={e.extension} className="border-b border-border/60 last:border-0">
                    <td className="py-1.5 font-mono text-[12px]">
                      {e.extension ? `.${e.extension}` : "(none)"}
                    </td>
                    <td className="py-1.5 text-right text-muted">{formatCount(e.files)}</td>
                    <td className="py-1.5 text-right">{formatBytes(e.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>

      <Card>
        <CardHeader title="Largest files" />
        <ul className="px-2 pb-2">
          {a?.largestFiles.map((f) => (
            <li key={f.id}>
              <button
                className="flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left hover:bg-surface-2"
                onClick={() => actions.details(f)}
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px]">{f.name}</span>
                  <span className="block truncate text-[11.5px] text-subtle">{f.parent}</span>
                </span>
                <span className="tabular text-[12.5px]">{formatBytes(f.size)}</span>
              </button>
            </li>
          ))}
        </ul>
      </Card>
    </Page>
  );
}
