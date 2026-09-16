import { describe, expect, test } from "bun:test";
import {
  TAG_PALETTE_KEYS,
  TAG_PILL_STYLES,
  nextColor,
  tagPillClass,
} from "../../src/lib/meeting-tags";
import paletteManifest from "../../src/lib/tag-palette.json";

describe("tag palette", () => {
  test("exposes at least 30 distinct keys", () => {
    expect(TAG_PALETTE_KEYS.length).toBeGreaterThanOrEqual(30);
    expect(new Set(TAG_PALETTE_KEYS).size).toBe(TAG_PALETTE_KEYS.length);
  });

  test("frontend keys match the shared manifest", () => {
    expect([...TAG_PALETTE_KEYS].sort()).toEqual([...paletteManifest].sort());
  });

  test("every key maps to a distinct chip class", () => {
    const classes = TAG_PALETTE_KEYS.map((key) => TAG_PILL_STYLES[key]);
    expect(classes.every(Boolean)).toBe(true);
    expect(new Set(classes).size).toBe(classes.length);
    expect(tagPillClass(TAG_PALETTE_KEYS[0])).toBe(TAG_PILL_STYLES[TAG_PALETTE_KEYS[0]]);
  });

  test("manual cycle traverses the whole palette and wraps", () => {
    let color = TAG_PALETTE_KEYS[0];
    const seen = new Set([color]);
    for (let i = 1; i < TAG_PALETTE_KEYS.length; i += 1) {
      color = nextColor(color);
      seen.add(color);
    }
    expect(seen.size).toBe(TAG_PALETTE_KEYS.length);
    expect(nextColor(TAG_PALETTE_KEYS[TAG_PALETTE_KEYS.length - 1])).toBe(
      TAG_PALETTE_KEYS[0],
    );
  });
});
