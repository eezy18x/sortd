// Formatting helpers. Pure functions; covered by unit tests.

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** Human-readable size using binary multiples (1 KB = 1024 B). */
export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[i]}`;
}

export function formatCount(n: number): string {
  return new Intl.NumberFormat().format(n);
}

/** Compact count: 1.2K, 3.4M. */
export function formatCompact(n: number): string {
  return new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(
    n,
  );
}

export function formatPercent(fraction: number, digits = 0): string {
  if (!Number.isFinite(fraction)) return "—";
  return `${(fraction * 100).toFixed(digits)}%`;
}

/** Absolute date/time from Unix seconds. */
export function formatDate(unixSeconds: number | null | undefined, withTime = false): string {
  if (unixSeconds == null) return "—";
  const d = new Date(unixSeconds * 1000);
  return withTime
    ? d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
    : d.toLocaleDateString(undefined, { dateStyle: "medium" });
}

/** "3 minutes ago", "in 2 days" etc. `now` is injectable for tests. */
export function formatRelative(unixSeconds: number | null | undefined, now = Date.now()): string {
  if (unixSeconds == null) return "—";
  const diff = unixSeconds * 1000 - now;
  const abs = Math.abs(diff);
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  const table: [number, Intl.RelativeTimeFormatUnit][] = [
    [60_000, "second"],
    [3_600_000, "minute"],
    [86_400_000, "hour"],
    [604_800_000, "day"],
    [2_629_800_000, "week"],
    [31_557_600_000, "month"],
    [Infinity, "year"],
  ];
  const divisors: Record<string, number> = {
    second: 1000,
    minute: 60_000,
    hour: 3_600_000,
    day: 86_400_000,
    week: 604_800_000,
    month: 2_629_800_000,
    year: 31_557_600_000,
  };
  for (const [limit, unit] of table) {
    if (abs < limit) return rtf.format(Math.round(diff / divisors[unit]!), unit);
  }
  return formatDate(unixSeconds);
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} s`;
  const m = Math.floor(s / 60);
  const rem = Math.round(s % 60);
  if (m < 60) return `${m}m ${rem}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** Splits a path into its final component and the containing folder. */
export function splitPath(path: string): { dir: string; name: string } {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (idx < 0) return { dir: "", name: path };
  return { dir: path.slice(0, idx), name: path.slice(idx + 1) };
}

/** Path relative to `base` when it lies below it; otherwise the full path. */
export function relativeTo(path: string, base: string): string {
  const norm = (p: string) => p.replace(/[\\/]+$/, "");
  const b = norm(base);
  if (path.length > b.length && path.startsWith(b) && /[\\/]/.test(path[b.length]!)) {
    return path.slice(b.length + 1);
  }
  return path;
}

/** Parses "10 MB", "1.5gb", "500" (bytes) into bytes. */
export function parseSize(input: string): number | null {
  const m = input
    .trim()
    .toLowerCase()
    .match(/^(\d+(?:\.\d+)?)\s*(b|kb?|mb?|gb?|tb?)?$/);
  if (!m) return null;
  const n = Number(m[1]);
  const unit = (m[2] ?? "b")[0];
  const mult = { b: 1, k: 1024, m: 1024 ** 2, g: 1024 ** 3, t: 1024 ** 4 }[unit as "b"] ?? 1;
  return Math.round(n * mult);
}

export const nowSeconds = (): number => Math.floor(Date.now() / 1000);
export const DAY = 86_400;
