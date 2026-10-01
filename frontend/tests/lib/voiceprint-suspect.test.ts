import { describe, expect, test } from "bun:test";
import { hasSuspect, suspectBadge, visibleRows } from "../../src/lib/voiceprint-suspect";

const row = (id: string, is_verified: number, suspect?: boolean) => ({ id, is_verified, suspect });

describe("suspectBadge", () => {
  test("an unverified suspect row shows the suspect badge", () => {
    expect(suspectBadge(row("a", 0, true))).toBe("suspect");
  });

  test("a verified suspect row shows as acknowledged, not removed", () => {
    expect(suspectBadge(row("a", 1, true))).toBe("acknowledged");
  });

  test("a row without the flag shows nothing", () => {
    expect(suspectBadge(row("a", 0, false))).toBeNull();
    expect(suspectBadge(row("a", 0))).toBeNull();
  });
});

describe("visibleRows", () => {
  const rows = [row("ok", 0, false), row("odd", 0, true), row("odd-verified", 1, true), row("done", 1, false)];

  test("no filter keeps every row", () => {
    expect(visibleRows(rows, { hideVerified: false, suspectOnly: false }).map((r) => r.id)).toEqual([
      "ok",
      "odd",
      "odd-verified",
      "done",
    ]);
  });

  test("the suspect filter lists only suspects, verified ones included", () => {
    expect(visibleRows(rows, { hideVerified: false, suspectOnly: true }).map((r) => r.id)).toEqual([
      "odd",
      "odd-verified",
    ]);
  });

  test("suspect filter and hide-verified combine", () => {
    expect(visibleRows(rows, { hideVerified: true, suspectOnly: true }).map((r) => r.id)).toEqual(["odd"]);
  });
});

describe("hasSuspect", () => {
  test("is true only when some row is flagged", () => {
    expect(hasSuspect([row("a", 0, false), row("b", 0, true)])).toBe(true);
    expect(hasSuspect([row("a", 0, false)])).toBe(false);
    expect(hasSuspect([])).toBe(false);
  });
});
