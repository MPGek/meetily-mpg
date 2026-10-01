import { describe, expect, test } from "bun:test";
import { formatLegacySummaryData, isLegacySummaryEmpty } from "../../src/lib/summary-formatting";

describe("isLegacySummaryEmpty", () => {
  test("true when every section's blocks is missing or empty", () => {
    expect(isLegacySummaryEmpty([["a", { title: "A" }], ["b", { title: "B", blocks: [] }]])).toBe(true);
  });

  test("false when any section has a block", () => {
    expect(
      isLegacySummaryEmpty([["a", { blocks: [] }], ["b", { blocks: [{ content: "x" }] }]])
    ).toBe(false);
  });

  test("true for no sections", () => {
    expect(isLegacySummaryEmpty([])).toBe(true);
  });
});

describe("formatLegacySummaryData", () => {
  const data = {
    Second: { title: "Second title", blocks: [{ id: "b1", type: "bullet", content: "  two  ", color: "red" }] },
    First: { title: "", blocks: [{ id: "a1", type: "text", content: "one" }] },
    NoBlocks: { title: "No blocks", blocks: null },
    _section_order: ["First", "Second", "NoBlocks"],
    notASection: "x",
  };

  test("visits keys in the given order", () => {
    const out = formatLegacySummaryData(["Second", "First"], data);
    expect(Object.keys(out)).toEqual(["Second", "First"]);
  });

  test("defaults a missing title to the section key", () => {
    const out = formatLegacySummaryData(["First"], data);
    expect(out.First.title).toBe("First");
  });

  test("trims block content and sets color 'default' on every block", () => {
    const out = formatLegacySummaryData(["First", "Second"], data);
    expect(out.Second.blocks).toEqual([{ id: "b1", type: "bullet", content: "two", color: "default" }]);
    expect(out.First.blocks).toEqual([{ id: "a1", type: "text", content: "one", color: "default" }]);
  });

  test("a section whose blocks is not an array gets an empty block list", () => {
    const out = formatLegacySummaryData(["NoBlocks"], data);
    expect(out.NoBlocks).toEqual({ title: "No blocks", blocks: [] });
  });

  test("non-section values and unknown keys are skipped", () => {
    const out = formatLegacySummaryData(["notASection", "_section_order", "missing"], data);
    expect(out).toEqual({});
  });
});
