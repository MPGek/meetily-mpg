import { describe, expect, test } from "bun:test";
import { dedupeAndInsertTranscript, formatTranscriptForClipboard } from "../../src/lib/transcript-formatting";
import type { Transcript } from "../../src/types";

const t = (id: string, text: string, timestamp: string, sequence_id?: number, audio_start_time?: number): Transcript =>
  ({ id, text, timestamp, sequence_id, audio_start_time });

describe("dedupeAndInsertTranscript", () => {
  test("appends a new transcript and keeps the list sorted by sequence_id", () => {
    const prev = [t("1", "a", "10:00:00", 1), t("3", "c", "10:00:02", 3)];
    const next = dedupeAndInsertTranscript(prev, t("2", "b", "10:00:01", 2));
    expect(next.map((x) => x.id)).toEqual(["1", "2", "3"]);
    expect(prev).toHaveLength(2);
  });

  test("a missing sequence_id sorts as 0", () => {
    const next = dedupeAndInsertTranscript([t("1", "a", "10:00:00", 1)], t("x", "b", "10:00:01"));
    expect(next.map((x) => x.id)).toEqual(["x", "1"]);
  });

  test("a transcript with the same text and timestamp is dropped and prev is returned", () => {
    const prev = [t("1", "a", "10:00:00", 1)];
    const next = dedupeAndInsertTranscript(prev, t("9", "a", "10:00:00", 9));
    expect(next).toBe(prev);
  });

  test("same text with a different timestamp is not a duplicate", () => {
    const next = dedupeAndInsertTranscript([t("1", "a", "10:00:00", 1)], t("2", "a", "10:00:05", 2));
    expect(next).toHaveLength(2);
  });
});

describe("formatTranscriptForClipboard", () => {
  test("produces one [MM:SS] text line per transcript", () => {
    const out = formatTranscriptForClipboard([
      t("1", "hello", "x", 1, 5.9),
      t("2", "world", "x", 2, 125.3),
    ]);
    expect(out).toBe("[00:05] hello\n[02:05] world");
  });

  test("an undefined audio_start_time renders as [--:--]", () => {
    expect(formatTranscriptForClipboard([t("1", "hi", "x", 1)])).toBe("[--:--] hi");
  });

  test("an empty list gives an empty string", () => {
    expect(formatTranscriptForClipboard([])).toBe("");
  });
});
