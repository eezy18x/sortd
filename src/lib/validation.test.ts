import { describe, expect, it } from "vitest";
import { invalidNameReason, invalidSubpathReason, joinSubpath } from "./validation";
import { CATEGORIES, categoryMeta } from "./categories";

describe("invalidNameReason", () => {
  it("accepts ordinary and Unicode names", () => {
    expect(invalidNameReason("report.pdf")).toBeNull();
    expect(invalidNameReason("日本語 レポート.docx")).toBeNull();
  });
  it("rejects separators, reserved and control characters", () => {
    expect(invalidNameReason("")).not.toBeNull();
    expect(invalidNameReason("..")).not.toBeNull();
    expect(invalidNameReason("a/b")).not.toBeNull();
    expect(invalidNameReason("a\\b")).not.toBeNull();
    expect(invalidNameReason("what?")).not.toBeNull();
    expect(invalidNameReason("bad\nname")).not.toBeNull();
    expect(invalidNameReason("CON.txt")).not.toBeNull();
    expect(invalidNameReason("trailing.")).not.toBeNull();
  });
});

describe("subpaths", () => {
  it("validates every component and blocks traversal", () => {
    expect(invalidSubpathReason("Reports/2024")).toBeNull();
    expect(invalidSubpathReason("../escape")).not.toBeNull();
    expect(invalidSubpathReason("a/../b")).not.toBeNull();
  });
  it("joins with the base path's separator", () => {
    expect(joinSubpath("C:\\Docs", "Reports/2024")).toBe("C:\\Docs\\Reports\\2024");
    expect(joinSubpath("/home/a/", "x\\y")).toBe("/home/a/x/y");
    expect(joinSubpath("/home/a", "  ")).toBe("/home/a");
  });
});

describe("categories", () => {
  it("has metadata for every backend category and a safe fallback", () => {
    expect(CATEGORIES).toHaveLength(13);
    expect(categoryMeta("images").label).toBe("Images");
    expect(categoryMeta("nonsense").id).toBe("other");
  });
});
