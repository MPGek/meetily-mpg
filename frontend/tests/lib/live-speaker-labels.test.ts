import { describe, expect, test } from "bun:test";
import {
  matchSpeakerToTranscript,
  rewriteTurnsForBinding,
  rewriteTurnsInWindow,
  rematchTranscripts,
  upsertLiveBlocks,
  resolveLiveBlocks,
} from "../../src/lib/live-speaker-labels";
import type { SpeakerTurn } from "../../src/services/recordingService";
import type { LiveTranscriptBlock, LiveTranscriptBlocks, Transcript } from "../../src/types";

const turn = (partial: Partial<SpeakerTurn>): SpeakerTurn => ({
  start_time: 0,
  end_time: 1,
  speaker: "SPEAKER_00",
  source_device: "System",
  ...partial,
});

const transcript = (partial: Partial<Transcript>): Transcript => ({
  id: "t-1",
  text: "hello",
  timestamp: "00:00",
  audio_start_time: 0,
  audio_end_time: 2,
  duration: 2,
  source_device: "System",
  ...partial,
});

describe("matchSpeakerToTranscript", () => {
  test("selects the turn with greatest temporal overlap on the same channel", () => {
    const turns = [
      turn({ start_time: 0, end_time: 1, speaker: "SPEAKER_01", source_device: "System" }),
      turn({ start_time: 1, end_time: 2, speaker: "SPEAKER_02", source_device: "Microphone" }),
    ];
    const seg = transcript({ audio_start_time: 0, audio_end_time: 3, source_device: "System" });
    const m = matchSpeakerToTranscript(seg, turns);
    expect(m?.speaker).toBe("SPEAKER_01");
  });

  test("returns undefined when no turn overlaps", () => {
    const seg = transcript({ audio_start_time: 10, audio_end_time: 11 });
    const m = matchSpeakerToTranscript(seg, [turn({ start_time: 0, end_time: 1 })]);
    expect(m).toBeUndefined();
  });
});

describe("rewriteTurnsForBinding", () => {
  test("replaces every turn of the cluster with the bound name + user provenance", () => {
    const turns = [
      turn({ speaker: "SPEAKER_01", display_name: "Bob (auto)", matched_by: "auto", match_score: 0.88 }),
      turn({ speaker: "SPEAKER_02", display_name: "Carol", matched_by: "auto" }),
      turn({ speaker: "SPEAKER_01", display_name: undefined }),
    ];
    const out = rewriteTurnsForBinding(turns, "SPEAKER_01", "Alice");
    expect(out[0]).toMatchObject({ display_name: "Alice", matched_by: "user" });
    expect(out[0].match_score).toBeUndefined();
    expect(out[1]).toMatchObject({ display_name: "Carol", matched_by: "auto" });
    expect(out[2]).toMatchObject({ display_name: "Alice", matched_by: "user" });
  });

  test("is idempotent: applying twice yields the same result", () => {
    const turns = [turn({ speaker: "SPEAKER_01", display_name: "Bob", matched_by: "auto" })];
    const once = rewriteTurnsForBinding(turns, "SPEAKER_01", "Alice");
    const twice = rewriteTurnsForBinding(once, "SPEAKER_01", "Alice");
    expect(twice).toEqual(once);
    expect(twice[0]).toMatchObject({ display_name: "Alice", matched_by: "user" });
  });
});

describe("rewriteTurnsInWindow", () => {
  test("rewrites only the turn(s) overlapping the window and same channel", () => {
    const turns = [
      turn({ start_time: 0, end_time: 4, speaker: "SPEAKER_01", source_device: "System" }),
      turn({ start_time: 8, end_time: 10, speaker: "SPEAKER_01", source_device: "System" }),
      turn({ start_time: 0, end_time: 4, speaker: "SPEAKER_01", source_device: "Microphone" }),
      turn({ start_time: 2, end_time: 5, speaker: "SPEAKER_02", source_device: "System" }),
    ];
    const out = rewriteTurnsInWindow(turns, "SPEAKER_01", "System", 0, 5, "Alice");
    // overlapping system SPEAKER_01 turn rewritten
    expect(out[0]).toMatchObject({ display_name: "Alice", matched_by: "user" });
    // non-overlapping same-cluster turn left unchanged
    expect(out[1].display_name).toBeUndefined();
    expect(out[1].matched_by).toBeUndefined();
    // wrong channel left unchanged
    expect(out[2].display_name).toBeUndefined();
    // different cluster left unchanged
    expect(out[3].display_name).toBeUndefined();
  });
});

describe("rematchTranscripts freeze", () => {
  test("a user-assigned transcript keeps its label when an unrelated turn arrives, even with a stale auto turn present", () => {
    // transcript T pinned to SPEAKER_01 -> Alice
    const segments = [
      transcript({ id: "T", audio_start_time: 0, audio_end_time: 3, source_device: "System", speaker: "SPEAKER_01", speaker_label: "Alice", speaker_matched_by: "user" }),
      transcript({ id: "U", audio_start_time: 10, audio_end_time: 12, source_device: "System", speaker: "SPEAKER_00" }),
    ];
    const assigned = new Map([["T", { cluster: "SPEAKER_01", name: "Alice" }]]);

    // turnsRef still holds the STALE auto turn for SPEAKER_01, and a new
    // unrelated SPEAKER_02 turn arrives.
    const turns = [
      turn({ start_time: 0, end_time: 3, speaker: "SPEAKER_01", display_name: "Bob (auto)", matched_by: "auto", match_score: 0.9 }),
      turn({ start_time: 20, end_time: 21, speaker: "SPEAKER_02", display_name: "Carol", matched_by: "auto" }),
    ];

    const out = rematchTranscripts(segments, turns, assigned);
    const t = out.find(s => s.id === "T")!;
    const u = out.find(s => s.id === "U")!;
    // pinned transcript is untouched — no revert to the stale auto name
    expect(t.speaker_label).toBe("Alice");
    expect(t.speaker_matched_by).toBe("user");
    // unpinned transcript still re-matches (retroactive fill-in preserved)
    expect(u.speaker).toBe("SPEAKER_00");
  });

  test("a transcript frozen by a sub-row edit is not re-matched, though nothing is pinned", () => {
    const segments = [transcript({ id: "T", audio_start_time: 0, audio_end_time: 3, speaker: "SPEAKER_01", speaker_label: "Alice", speaker_matched_by: "auto" })];
    const turns = [turn({ start_time: 0, end_time: 3, speaker: "SPEAKER_04", display_name: "Bob", matched_by: "auto" })];
    const out = rematchTranscripts(segments, turns, new Set(["T"]));
    expect(out).toBe(segments);
  });

  test("unpinned transcripts still get retroactive label fill-in", () => {
    const segments = [transcript({ id: "U", audio_start_time: 0, audio_end_time: 3, source_device: "System" })];
    const assigned = new Map<string, unknown>();
    const turns = [turn({ start_time: 0, end_time: 3, speaker: "SPEAKER_07", display_name: "Dana", matched_by: "auto", match_score: 0.8 })];
    const out = rematchTranscripts(segments, turns, assigned);
    expect(out[0].speaker).toBe("SPEAKER_07");
    expect(out[0].speaker_label).toBe("Dana");
    expect(out[0].speaker_matched_by).toBe("auto");
  });
});

const liveBlocks = (partial: Partial<LiveTranscriptBlocks>): LiveTranscriptBlocks => ({
  parent_sequence_id: 7,
  source_device: "Microphone",
  revision: 1,
  blocks: [],
  final: false,
  ...partial,
});

const block = (partial: Partial<LiveTranscriptBlock>): LiveTranscriptBlock => ({
  start: 0,
  end: 1,
  text: "hi",
  speaker: "SPEAKER_00",
  ...partial,
});

describe("upsertLiveBlocks", () => {
  test("a newer revision replaces the previous rendering (no duplicate rows)", () => {
    const first = upsertLiveBlocks(new Map(), liveBlocks({ revision: 1 }));
    const second = upsertLiveBlocks(first, { ...liveBlocks({ revision: 2 }), blocks: [block({}), block({ start: 1, end: 2, speaker: "SPEAKER_01" })] });
    expect(second.size).toBe(1);
    expect(second.get(7)?.revision).toBe(2);
    expect(second.get(7)?.blocks).toHaveLength(2);
  });

  test("a stale revision is ignored", () => {
    const current = upsertLiveBlocks(new Map(), liveBlocks({ revision: 3 }));
    const out = upsertLiveBlocks(current, liveBlocks({ revision: 2 }));
    expect(out).toBe(current);
    expect(out.get(7)?.revision).toBe(3);
  });
});

describe("resolveLiveBlocks across a live split", () => {
  const split = [
    block({ start: 0, end: 1, speaker: "SPEAKER_00", text: "A" }),
    block({ start: 1, end: 2, speaker: "SPEAKER_01", text: "B" }),
  ];

  test("pinned cluster label applies to every sub-row of that cluster", () => {
    const out = resolveLiveBlocks(split, { pinned: { cluster: "SPEAKER_01", name: "Alice" } });
    expect(out[0].display_name).toBeUndefined();
    expect(out[1]).toMatchObject({ display_name: "Alice", matched_by: "user" });
  });

  test("a window-scoped override lands only on the covering sub-row", () => {
    const out = resolveLiveBlocks(split, {
      windowOverrides: [{ cluster: "SPEAKER_01", start: 1, end: 2, name: "Bob" }],
    });
    expect(out[0].display_name).toBeUndefined();
    expect(out[1]).toMatchObject({ display_name: "Bob", matched_by: "user" });
  });

  test("a cluster binding applies to all sub-rows of the cluster", () => {
    const out = resolveLiveBlocks(split, {
      clusterBindings: new Map([["SPEAKER_00", "Carol"]]),
    });
    expect(out[0]).toMatchObject({ display_name: "Carol", matched_by: "user" });
    expect(out[1].display_name).toBeUndefined();
  });

  test("an override from another channel leaves the auto display name in place", () => {
    const withAuto = [block({ start: 0, end: 1, speaker: "SPEAKER_00", display_name: "Dana", matched_by: "auto" })];
    const out = resolveLiveBlocks(withAuto, {
      sourceDevice: "System",
      windowOverrides: [{ cluster: "MIC_SPEAKER_09", sourceDevice: "Microphone", start: 0, end: 1, name: "Eve" }],
    });
    expect(out[0]).toMatchObject({ display_name: "Dana", matched_by: "auto" });
  });
});

describe("resolveLiveBlocks sub-row scope", () => {
  // A, B, A: two sub-rows of one cluster around a second cluster, all auto.
  const aba = [
    block({ start: 0, end: 1, speaker: "SPEAKER_00", display_name: "Alice", matched_by: "auto", match_score: 0.9 }),
    block({ start: 1, end: 2, speaker: "SPEAKER_01", display_name: "Alice", matched_by: "auto", match_score: 0.9 }),
    block({ start: 2, end: 3, speaker: "SPEAKER_00", display_name: "Alice", matched_by: "auto", match_score: 0.9 }),
  ];
  const ov = (start: number, end: number, name: string, cluster = "SPEAKER_00") =>
    ({ cluster, sourceDevice: "System", start, end, name });

  test("editing one sub-row leaves same-cluster siblings alone", () => {
    const out = resolveLiveBlocks(aba, { sourceDevice: "System", windowOverrides: [ov(0, 1, "Bob")] });
    expect(out[0]).toMatchObject({ display_name: "Bob", matched_by: "user" });
    expect(out[2]).toMatchObject({ display_name: "Alice", matched_by: "auto" });
    expect(out[2]).toBe(aba[2]);
  });

  test("a later sub-row edit does not revert an earlier one", () => {
    const out = resolveLiveBlocks(aba, {
      sourceDevice: "System",
      windowOverrides: [ov(1, 2, "Bob", "SPEAKER_01"), ov(2, 3, "Alice")],
    });
    expect(out[1]).toMatchObject({ display_name: "Bob", matched_by: "user" });
    expect(out[2]).toMatchObject({ display_name: "Alice", matched_by: "user" });
  });

  test("confirm changes provenance only on the confirmed sub-row", () => {
    const out = resolveLiveBlocks(aba, { sourceDevice: "System", windowOverrides: [ov(2, 3, "Alice")] });
    expect(out.map(b => b.matched_by)).toEqual(["auto", "auto", "user"]);
  });

  test("re-attribution to another cluster keeps the override", () => {
    const reattributed = [block({ start: 0, end: 1, speaker: "SPEAKER_05", display_name: "Carl", matched_by: "auto" })];
    const out = resolveLiveBlocks(reattributed, { sourceDevice: "System", windowOverrides: [ov(0, 1, "Bob")] });
    expect(out[0]).toMatchObject({ display_name: "Bob", matched_by: "user" });
  });

  test("a shifted boundary does not spill an override onto the neighbour", () => {
    const shifted = [
      block({ start: 0, end: 0.9, speaker: "SPEAKER_00", display_name: "Alice", matched_by: "auto" }),
      block({ start: 0.9, end: 2, speaker: "SPEAKER_01", display_name: "Alice", matched_by: "auto" }),
    ];
    const out = resolveLiveBlocks(shifted, { sourceDevice: "System", windowOverrides: [ov(0, 1, "Bob")] });
    expect(out[0].display_name).toBe("Bob");
    expect(out[1]).toMatchObject({ display_name: "Alice", matched_by: "auto" });
  });

  test("the latest override wins", () => {
    const out = resolveLiveBlocks(aba, {
      sourceDevice: "System",
      windowOverrides: [ov(0, 1, "Bob"), ov(0, 1.1, "Carol")],
    });
    expect(out[0].display_name).toBe("Carol");
  });

  test("a pin from before the split still applies after a sub-row edit", () => {
    const out = resolveLiveBlocks(aba, {
      sourceDevice: "System",
      pinned: { cluster: "SPEAKER_00", name: "Alice" },
      windowOverrides: [ov(1, 2, "Bob", "SPEAKER_01")],
    });
    expect(out[0]).toMatchObject({ display_name: "Alice", matched_by: "user" });
    expect(out[1]).toMatchObject({ display_name: "Bob", matched_by: "user" });
    expect(out[2]).toMatchObject({ display_name: "Alice", matched_by: "user" });
  });
});
