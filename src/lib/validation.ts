// Client-side name validation. This is a convenience for immediate feedback
// only; the backend re-validates every name and path authoritatively.

const FORBIDDEN = new Set(["\\", "/", "<", ">", ":", '"', "|", "?", "*"]);

function hasControlChars(s: string): boolean {
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    if (c < 0x20 || c === 0x7f) return true;
  }
  return false;
}

/** Returns a reason when `name` is not a portable file or folder name. */
export function invalidNameReason(name: string): string | null {
  const n = name.trim();
  if (!n) return "Enter a name";
  if (n === "." || n === "..") return "That name is reserved";
  if (hasControlChars(n)) return "Names can’t contain control characters";
  for (const ch of n) if (FORBIDDEN.has(ch)) return `Names can’t contain ${ch}`;
  if (/[. ]$/.test(n)) return "Names can’t end with a dot or space";
  if (/^(con|prn|aux|nul|com\d|lpt\d)(\..*)?$/i.test(n)) return "That name is reserved on Windows";
  return null;
}

/** Validates a relative subpath such as "Reports/2024" (each part must be a valid name). */
export function invalidSubpathReason(sub: string): string | null {
  const parts = sub.split(/[\\/]+/).filter((p) => p.trim() !== "");
  for (const p of parts) {
    const r = invalidNameReason(p);
    if (r) return r;
  }
  return null;
}

/** Joins a base path and a user-typed relative subpath ("a/b" or "a\\b"). */
export function joinSubpath(base: string, sub: string): string {
  const sep = base.includes("\\") ? "\\" : "/";
  const parts = sub
    .split(/[\\/]+/)
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.length === 0) return base;
  return base.replace(/[\\/]+$/, "") + sep + parts.join(sep);
}
