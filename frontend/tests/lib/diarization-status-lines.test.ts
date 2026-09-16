import { describe, expect, test } from "bun:test";
import {
  buildChannelLine,
  buildLevelBar,
  buildModelIndicators,
  DIAR_STATE_CLASS,
  DIAR_STATE_TEXT,
  LEVEL_STALE_MS,
  MODEL_BLINK_CLASS,
  MODEL_STATE_CLASS,
} from "../../src/lib/diarization-status-lines";
import type {
  AlignmentActivity,
  AsrActivity,
  DiarChannelState,
  DiarizationModelActivity,
  VadActivity,
} from "../../src/services/diarizationStatusService";

const vad = (partial: Partial<VadActivity> = {}): VadActivity => ({
  identity: "silero_vad_v6",
  loaded: true,
  mic_frames: 40,
  mic_speaking: true,
  sys_frames: 12,
  sys_speaking: false,
  speaking: true,
  ...partial,
});

const asr = (partial: Partial<AsrActivity> = {}): AsrActivity => ({
  engine: "Whisper",
  model: "large-v3-turbo",
  loaded: true,
  queued: 12,
  completed: 9,
  pending: 3,
  in_flight: false,
  requested: true,
  last_text: "hello world",
  ...partial,
});

const alignment = (
  partial: Partial<AlignmentActivity> = {}
): AlignmentActivity => ({
  enabled: true,
  model_id: "wav2vec2-xlsr-56",
  loaded: false,
  queued_jobs: 2,
  queue_bytes: 1024,
  dropped: 0,
  refined: 5,
  in_flight: false,
  requested: true,
  ...partial,
});

const diarizationModel = (
  partial: Partial<DiarizationModelActivity> = {}
): DiarizationModelActivity => ({
  mode: "fast",
  model_tag: "titanet_large",
  embedding_dim: 192,
  recognition_threshold: 0.68,
  loaded: true,
  prototypes: 128,
  bindings: 3,
  pending_blocks: 0,
  blocks_sent: 0,
  blocks_completed: 0,
  in_flight: false,
  requested: false,
  ...partial,
});

describe("buildChannelLine", () => {
  test("a ready channel shows its state text", () => {
    expect(buildChannelLine({ state: "accumulating" })).toBe(
      DIAR_STATE_TEXT.accumulating
    );
    expect(buildChannelLine({ state: "healthy" })).toBe(DIAR_STATE_TEXT.healthy);
  });

  test("mono and unavailable channels are named, not counted", () => {
    expect(buildChannelLine({ state: "inactive" })).toBe("mono session");
    expect(buildChannelLine({ state: "unavailable" })).toBe("unavailable");
  });
});

describe("buildLevelBar", () => {
  test("a normal speech level fills the meter", () => {
    const bar = buildLevelBar({ rms: 0.05, peak: 0.2, age_ms: 20 });
    expect(bar.percent).toBeGreaterThan(30);
    expect(bar.percent).toBeLessThanOrEqual(100);
    expect(bar.detail).toContain("dBFS");
  });

  test("a louder channel fills more of the meter", () => {
    const quiet = buildLevelBar({ rms: 0.01, peak: 0.05, age_ms: 20 });
    const loud = buildLevelBar({ rms: 0.3, peak: 0.6, age_ms: 20 });
    expect(loud.percent).toBeGreaterThan(quiet.percent);
  });

  test("silence and room noise read as empty", () => {
    expect(buildLevelBar({ rms: 0, peak: 0, age_ms: 20 }).percent).toBe(0);
    expect(buildLevelBar({ rms: 0.0002, peak: 0.001, age_ms: 20 }).percent).toBe(0);
  });

  test("a level older than the staleness window reads as empty, not held", () => {
    const fresh = buildLevelBar({ rms: 0.3, peak: 0.6, age_ms: 20 });
    const stale = buildLevelBar({ rms: 0.3, peak: 0.6, age_ms: 900 });
    expect(fresh.percent).toBeGreaterThan(0);
    expect(stale.percent).toBe(0);
    expect(stale.detail).toContain("no audio");
  });

  test("a clipping peak is marked without being treated as an error", () => {
    const clipping = buildLevelBar({ rms: 0.9, peak: 1, age_ms: 20 });
    expect(clipping.firing).toBe(true);
    // A clipped signal still has an RMS just below full scale.
    expect(clipping.percent).toBeGreaterThanOrEqual(95);
    expect(clipping.detail).toContain("clipping");
    // A stale sample never shows as clipping.
    expect(buildLevelBar({ rms: 0.9, peak: 1, age_ms: 900 }).firing).toBe(false);
  });
});

describe("buildModelIndicators", () => {
  const healthy = (states: Parameters<typeof buildModelIndicators>[4]) =>
    buildModelIndicators(
      vad(),
      asr({ in_flight: true, requested: false }),
      alignment({ loaded: true, in_flight: true, requested: false }),
      diarizationModel({ pending_blocks: 2, in_flight: true }),
      states
    );

  test("shows one labelled indicator per model kind", () => {
    const indicators = healthy(["healthy", "healthy"]);
    expect(indicators.map((i) => i.key)).toEqual(["vad", "asr", "align", "diar"]);
    expect(indicators.map((i) => i.label)).toEqual(["VAD", "ASR", "ALIGN", "DIAR"]);
    for (const indicator of indicators) {
      expect(indicator.title).toBeTruthy();
      expect(indicator.state).toBe("healthy");
    }
  });

  test("an unloaded model is idle, never healthy", () => {
    const indicators = buildModelIndicators(
      vad({ loaded: false }),
      asr({ engine: null, model: null, loaded: false }),
      alignment({ loaded: false }),
      diarizationModel(),
      ["accumulating", "accumulating"]
    );
    expect(indicators[0].state).toBe("idle");
    expect(indicators[1].state).toBe("idle");
    expect(indicators[2].state).toBe("idle");
    expect(indicators[3].state).toBe("healthy");
  });

  test("a disabled model is idle, not an error", () => {
    const indicators = buildModelIndicators(
      vad(),
      asr(),
      alignment({ enabled: false, loaded: false }),
      diarizationModel({ mode: "off", loaded: false }),
      ["inactive", "inactive"]
    );
    expect(indicators[2].state).toBe("idle");
    expect(indicators[3].state).toBe("idle");
    expect(indicators[2].state).not.toBe("error");
    expect(indicators[3].state).not.toBe("error");
  });

  test("dropped alignment work is a warning", () => {
    const indicators = buildModelIndicators(
      vad(),
      asr(),
      alignment({ dropped: 3 }),
      diarizationModel(),
      ["healthy", "healthy"]
    );
    expect(indicators[2].state).toBe("warning");
  });

  test("a failed diarization engine is an error, and turn-order break is a warning", () => {
    const failed = healthy(["error", "accumulating"]);
    expect(failed[3].state).toBe("error");

    const broken = healthy(["warning", "healthy"]);
    expect(broken[3].state).toBe("warning");
  });
});

describe("blink states", () => {
  test("a model actively processing blinks green", () => {
    const indicators = buildModelIndicators(
      vad({ speaking: true }),
      asr({ in_flight: true, requested: false }),
      alignment({ loaded: true, in_flight: true, requested: false }),
      diarizationModel({ pending_blocks: 2, in_flight: true }),
      ["healthy", "healthy"]
    );
    expect(indicators.map((i) => i.blink)).toEqual(["green", "green", "green", "green"]);
  });

  test("a queued request the model has not started consuming blinks red", () => {
    const indicators = buildModelIndicators(
      vad({ speaking: false }),
      asr({ in_flight: false, requested: true }),
      alignment({ loaded: true, in_flight: false, requested: true }),
      diarizationModel({ pending_blocks: 3, in_flight: false }),
      ["healthy", "healthy"]
    );
    expect(indicators[1].blink).toBe("red");
    expect(indicators[2].blink).toBe("red");
    expect(indicators[3].blink).toBe("red");
  });

  test("a loaded model without work is steady, not blinking", () => {
    const indicators = buildModelIndicators(
      vad({ speaking: false }),
      asr({ pending: 0, requested: false, queued: 9 }),
      alignment({ queued_jobs: 0, requested: false }),
      diarizationModel({ pending_blocks: 0, requested: false }),
      ["healthy", "healthy"]
    );
    for (const indicator of indicators) {
      expect(indicator.blink).toBe("none");
      expect(indicator.state).toBe("healthy");
    }
    expect(indicators.map((i) => i.pending)).toEqual([undefined, undefined, undefined, undefined]);
  });

  test("pending block counts are carried for STT, align and diarization", () => {
    const indicators = buildModelIndicators(
      vad({ speaking: false }),
      asr({ pending: 5, in_flight: true, requested: false }),
      alignment({ queued_jobs: 4, in_flight: false }),
      diarizationModel({ pending_blocks: 7, in_flight: true }),
      ["healthy", "healthy"]
    );
    expect(indicators[1].pending).toBe(5);
    expect(indicators[2].pending).toBe(4);
    expect(indicators[3].pending).toBe(7);
    expect(indicators[0].pending).toBeUndefined();
  });
});

describe("state mapping", () => {
  const states: DiarChannelState[] = [
    "unavailable",
    "inactive",
    "deferred",
    "accumulating",
    "healthy",
    "warning",
    "error",
  ];

  test("every state has text and a colour class", () => {
    for (const state of states) {
      expect(DIAR_STATE_TEXT[state]).toBeTruthy();
      expect(DIAR_STATE_CLASS[state]).toBeTruthy();
    }
  });

  test("model indicator colours are distinct and an idle model is grey, not red", () => {
    expect(MODEL_STATE_CLASS.healthy).toContain("green");
    expect(MODEL_STATE_CLASS.idle).toContain("gray");
    expect(MODEL_STATE_CLASS.warning).toContain("amber");
    expect(MODEL_STATE_CLASS.error).toContain("red");
    expect(MODEL_STATE_CLASS.idle).not.toContain("red");
  });

  test("blink classes pulse green or red", () => {
    expect(MODEL_BLINK_CLASS.none).toBe("");
    expect(MODEL_BLINK_CLASS.green).toContain("animate-pulse");
    expect(MODEL_BLINK_CLASS.green).toContain("green");
    expect(MODEL_BLINK_CLASS.red).toContain("animate-pulse");
    expect(MODEL_BLINK_CLASS.red).toContain("red");
  });

  test("error and warning are visually distinct treatments", () => {
    expect(DIAR_STATE_CLASS.error).not.toBe(DIAR_STATE_CLASS.warning);
    expect(DIAR_STATE_CLASS.error).not.toBe(DIAR_STATE_CLASS.healthy);
    expect(DIAR_STATE_CLASS.warning).not.toBe(DIAR_STATE_CLASS.healthy);
    expect(DIAR_STATE_CLASS.error).toContain("red");
    expect(DIAR_STATE_CLASS.warning).toContain("amber");
  });

  test("deferred and inactive read as expected, not as faults", () => {
    expect(DIAR_STATE_TEXT.deferred).toBe("cluster at stop");
    expect(DIAR_STATE_TEXT.inactive).toBe("mono");
    expect(DIAR_STATE_TEXT.error).toBe("ENGINE OFF");
  });
});
