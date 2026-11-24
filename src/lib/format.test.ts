import { describe, expect, it } from "vitest";
import {
  formatBytes,
  formatDuration,
  formatRelative,
  parseSize,
  relativeTo,
  splitPath,
} from "./format";

describe("formatBytes", () => {
  it("uses binary units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
    expect(formatBytes(150 * 1024 ** 2)).toBe("150 MB");
  });
  it("rejects invalid input", () => {
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
});

describe("parseSize", () => {
  it("parses units", () => {
    expect(parseSize("10 MB")).toBe(10 * 1024 ** 2);
    expect(parseSize("1.5gb")).toBe(Math.round(1.5 * 1024 ** 3));
    expect(parseSize("500")).toBe(500);
    expect(parseSize("2k")).toBe(2048);
  });
  it("returns null for garbage", () => {
    expect(parseSize("big")).toBeNull();
    expect(parseSize("10 parsecs")).toBeNull();
  });
});

describe("paths", () => {
  it("splits Windows and POSIX paths", () => {
    expect(splitPath("C:\\Users\\a\\report.pdf")).toEqual({
      dir: "C:\\Users\\a",
      name: "report.pdf",
    });
    expect(splitPath("/home/a/report.pdf")).toEqual({ dir: "/home/a", name: "report.pdf" });
    expect(splitPath("file")).toEqual({ dir: "", name: "file" });
  });
  it("computes relative paths only below the base", () => {
    expect(relativeTo("C:\\Users\\a\\Documents\\x", "C:\\Users\\a")).toBe("Documents\\x");
    expect(relativeTo("/home/ab/x", "/home/a")).toBe("/home/ab/x");
    expect(relativeTo("/home/a/x", "/home/a/")).toBe("x");
  });
});

describe("time", () => {
  it("formats relative times", () => {
    const now = Date.UTC(2026, 0, 10);
    expect(formatRelative(now / 1000 - 3 * 86400, now)).toMatch(/3 days ago/);
    expect(formatRelative(null, now)).toBe("—");
  });
  it("formats durations", () => {
    expect(formatDuration(250)).toBe("250 ms");
    expect(formatDuration(12_300)).toBe("12.3 s");
    expect(formatDuration(125_000)).toBe("2m 5s");
  });
});
