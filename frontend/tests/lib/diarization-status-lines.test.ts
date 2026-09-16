import { describe, expect, test } from "bun:test";
import {
  buildBufferBars,
  buildChannelLine,
  buildDiarContext,
  buildLevelBar,
  buildModelIndicators,
  buildModelLines,
  DIAR_STATE_CLASS,
  DIAR_STATE_TEXT,
  formatDiarTurn,
  formatPercent,
  formatSeconds,
  LEVEL_STALE_MS,
  MAX_DIAR_NAME_CHARS,
  MODEL_STATE_CLASS,
  truncateDiarName,
} from "../../src/lib/diarization-status-lines";
import type {
  AlignmentActivity,
  AsrActivity,
  ChannelPipelineFill,
  DiarChannelState,
  DiarChannelStatus,
  DiarizationModelActivity,
  VadActivity,
} from "../../src/services/diarizationStatusService";

const fill = (
  partial: Partial<ChannelPipelineFill> = {}
): ChannelPipelineFill => ({
  vad_dispatch: { fill: 4800, threshold: 9600, fraction: 0.5, fired: false },
  vad_frames: 40,
  vad_speaking: false,
  pending: {
    segments: 0,
    buffered_ms: 0,
    gap_trigger_ms: 500,
    cap_trigger_ms: 25_000,
  },
  mix: { fill: 0, threshold: 28_800, fraction: 0, fired: false },
  level: { rms: 0.05, peak: 0.2, age_ms: 20 },
  ...partial,
});

const line = (partial: Partial<DiarChannelStatus> = {}): DiarChannelStatus => ({
  channel: "microphone",
  state: "accumulating",
  chunks: 3,
  embed_ok: 3,
  embed_failed: 0,
  buffered: 3,
  buffered_secs: 12.4,
  turns: 8,
  ordered: true,
  ...partial,
});

const vad = (partial: Partial<VadActivity> = {}): VadActivity => ({
  identity: "silero_vad_v6",
  loaded: true,
  mic_frames: 40,
  mic_speaking: true,
  sys_frames: 12,
  sys_speaking: false,
  ...partial,
});

const asr = (partial: Partial<AsrActivity> = {}): AsrActivity => ({
  engine: "Whisper",
  model: "large-v3-turbo",
  loaded: true,
  queued: 12,
  completed: 9,
  pending: 3,
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
  ...partial,
});

describe("truncateDiarName", () => {
  test("keeps short names intact", () => {
    expect(truncateDiarName("Alice")).toBe("Alice");
  });

  test("caps long names at the limit", () => {
    const long = "Bartholomew Montgomery";
    const out = truncateDiarName(long);
    expect(out.length).toBe(MAX_DIAR_NAME_CHARS);
    expect(out.endsWith("\u2026")).toBe(true);
    expect(long.startsWith(out.slice(0, -1))).toBe(true);
  });
});

describe("formatDiarTurn", () => {
  test("prefers the display name over the raw label", () => {
    expect(
      formatDiarTurn({
        speaker: "MIC_SPEAKER_01",
        display_name: "Alice",
        matched_by: "auto",
        score: 0.74,
      })
    ).toBe("Alice auto 0.74");
  });

  test("falls back to the label and marks the attribution source", () => {
    expect(formatDiarTurn({ speaker: "SPEAKER_02", matched_by: "user" })).toBe(
      "SPEAKER_02 user"
    );
  });

  test("shows a dash when there is no attribution and omits a missing score", () => {
    expect(formatDiarTurn({ speaker: "SPEAKER_00" })).toBe("SPEAKER_00 -");
  });
});

describe("fill formatting", () => {
  test("percentages are whole numbers of the threshold", () => {
    expect(formatPercent(0)).toBe("0%");
    expect(formatPercent(0.5)).toBe("50%");
    expect(formatPercent(1)).toBe("100%");
    expect(formatPercent(1.5)).toBe("150%");
  });

  test("a missing threshold or nonsense fraction reads as zero", () => {
    expect(formatPercent(Number.NaN)).toBe("0%");
    expect(formatPercent(-1)).toBe("0%");
  });

  test("seconds keep one decimal", () => {
    expect(formatSeconds(0)).toBe("0.0s");
    expect(formatSeconds(3400)).toBe("3.4s");
    expect(formatSeconds(-500)).toBe("0.0s");
  });

});

describe("buildBufferBars", () => {
  test("each bar is labelled with the operation it gates", () => {
    const bars = buildBufferBars(fill());
    expect(bars.map((bar) => bar.label)).toEqual(["v", "p", "m"]);
  });

  test("a bar's length is the fill relative to its threshold", () => {
    const bars = buildBufferBars(fill());
    // vad_dispatch: 4800 / 9600
    expect(bars[0].percent).toBe(50);
    expect(bars[0].fired).toBe(false);
    // mix is empty in the fixture
    expect(bars[2].percent).toBe(0);
  });

  test("pending fills against its duration cap, not a fixed threshold", () => {
    const bars = buildBufferBars(
      fill({
        pending: {
          segments: 2,
          buffered_ms: 12_500,
          gap_trigger_ms: 500,
          cap_trigger_ms: 25_000,
        },
      })
    );
    expect(bars[1].percent).toBe(50);
    expect(bars[1].fired).toBe(false);
  });

  test("an over-threshold buffer renders full and still reports that it fired", () => {
    const bars = buildBufferBars(
      fill({
        vad_dispatch: { fill: 19_200, threshold: 9600, fraction: 2, fired: true },
      })
    );
    expect(bars[0].percent).toBe(100);
    expect(bars[0].fired).toBe(true);
  });

  test("a missing threshold renders an empty bar, not a full one", () => {
    const bars = buildBufferBars(
      fill({
        vad_dispatch: { fill: 0, threshold: 0, fraction: 0, fired: false },
        pending: {
          segments: 0,
          buffered_ms: 0,
          gap_trigger_ms: 0,
          cap_trigger_ms: 0,
        },
      })
    );
    expect(bars[0].percent).toBe(0);
    expect(bars[1].percent).toBe(0);
    expect(bars[1].fired).toBe(false);
  });
});

describe("buildChannelLine", () => {
  test("an idle channel reads as waiting, not as a fault", () => {
    const text = buildChannelLine(
      line({ chunks: 0, embed_ok: 0, buffered: 0, buffered_secs: 0, turns: 0 }),
      fill({ vad_speaking: false })
    );
    expect(text).toContain("0c");
    expect(text).toContain("0/0f");
    expect(text.endsWith(DIAR_STATE_TEXT.accumulating)).toBe(true);
    expect(text).not.toContain("ENGINE OFF");
    expect(text).not.toContain("ORDER BREAK");
  });

  test("counters, speech flag, buffered audio and the last turn are shown", () => {
    const text = buildChannelLine(
      line({
        last_turn: {
          speaker: "MIC_SPEAKER_01",
          display_name: "Artsiom Karan",
          matched_by: "auto",
          score: 0.69,
        },
      }),
      fill({ vad_speaking: true })
    );
    expect(text).toContain("3c");
    expect(text).toContain("3/0f");
    expect(text).toContain("sp");
    expect(text).toContain("12.4s");
    expect(text).toContain("8t");
    expect(text).toContain("Artsiom Karan auto 0.69");
  });

  test("the speech flag is omitted while the channel is silent", () => {
    expect(buildChannelLine(line(), fill({ vad_speaking: false }))).not.toContain(
      "sp"
    );
  });

  test("mono and unavailable channels are named, not counted", () => {
    expect(buildChannelLine(line({ state: "inactive" }), fill())).toBe(
      "mono session"
    );
    expect(buildChannelLine(line({ state: "unavailable" }), fill())).toBe(
      "unavailable"
    );
  });
});

describe("buildLevelBar", () => {
  test("a normal speech level fills the meter", () => {
    const bar = buildLevelBar({ rms: 0.05, peak: 0.2, age_ms: 20 });
    expect(bar.label).toBe("L");
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
    expect(clipping.fired).toBe(true);
    // A clipped signal still has an RMS just below full scale.
    expect(clipping.percent).toBeGreaterThanOrEqual(95);
    expect(clipping.detail).toContain("clipping");
    // A stale sample never shows as clipping.
    expect(buildLevelBar({ rms: 0.9, peak: 1, age_ms: 900 }).fired).toBe(false);
  });
});

describe("buildModelIndicators", () => {
  const healthy = (states: Parameters<typeof buildModelIndicators>[4]) =>
    buildModelIndicators(
      vad(),
      asr(),
      alignment({ loaded: true }),
      diarizationModel(),
      states
    );

  test("the meter is empty exactly when the sample is stale", () => {
    const level = { rms: 0.3, peak: 0.6 };
    expect(
      buildLevelBar({ ...level, age_ms: LEVEL_STALE_MS }).percent
    ).toBeGreaterThan(0);
    expect(
      buildLevelBar({ ...level, age_ms: LEVEL_STALE_MS + 1 }).percent
    ).toBe(0);
  });

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

describe("buildDiarContext", () => {
  test("always carries the recognition threshold that accepts a score", () => {
    const context = buildDiarContext(diarizationModel());
    expect(context).toContain("tau 0.68");
    expect(context).toContain("titanet_large 192d");
    expect(context).toContain("fast mode");
    expect(context).toContain("proto 128 / bind 3");
    expect(context).toContain("loaded");
  });

  test("reports unavailable prototypes instead of zeros", () => {
    const context = buildDiarContext(
      diarizationModel({ prototypes: null, bindings: null })
    );
    expect(context).toContain("prototypes unavailable");
    expect(context).not.toContain("proto 0");
  });

  test("an off session is described as off", () => {
    const context = buildDiarContext(
      diarizationModel({ mode: "off", loaded: false })
    );
    expect(context).toContain("off mode");
  });
});

const pipeline = { sample_rate: 48000, mic: fill() };

describe("buildModelLines", () => {
  test("names every model kind with readiness and activity", () => {
    const lines = buildModelLines(
      vad(),
      asr(),
      alignment(),
      diarizationModel(),
      pipeline
    );
    expect(lines.length).toBe(5);
    expect(lines[0]).toContain("VAD silero_vad_v6 loaded");
    expect(lines[0]).toContain("mic 40f sp");
    expect(lines[0]).toContain("sys 12f");
    expect(lines[1]).toContain("Whisper large-v3-turbo loaded");
    expect(lines[1]).toContain("queue 9/12");
    expect(lines[1]).toContain("3 pending");
    expect(lines[1]).toContain("last hello world");
    expect(lines[2]).toContain("ALIGN wav2vec2-xlsr-56 not loaded");
    expect(lines[2]).toContain("refined 5");
    // The diarization line carries the threshold needed to read a score.
    expect(lines[3]).toContain("tau 0.68");
    // The legend names what each gated buffer fires.
    expect(lines[4]).toContain("200ms voice-activity dispatch");
    expect(lines[4]).toContain("500ms gap or 25000ms cap");
    expect(lines[4]).toContain("600ms recording mix window");
  });

  test("idle recognition and disabled alignment are labelled, not failed", () => {
    const lines = buildModelLines(
      vad({ loaded: false, mic_frames: 0, mic_speaking: false }),
      asr({
        engine: null,
        model: null,
        loaded: false,
        queued: 0,
        completed: 0,
        pending: 0,
        last_text: null,
      }),
      alignment({ enabled: false, loaded: false, refined: 0, queued_jobs: 0 }),
      diarizationModel({ mode: "off", loaded: false, prototypes: null }),
      pipeline
    );
    expect(lines[0]).toContain("VAD silero_vad_v6 not loaded");
    expect(lines[1]).toBe("ASR: idle");
    expect(lines[2]).toBe("ALIGN: disabled");
  });

  test("queue overflow is surfaced", () => {
    const lines = buildModelLines(
      vad(),
      asr(),
      alignment({ dropped: 4 }),
      diarizationModel(),
      pipeline
    );
    expect(lines[2]).toContain("dropped 4");
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

  test("a filling buffer is never styled as an error or a warning", () => {
    for (const state of ["accumulating", "deferred", "healthy"] as const) {
      expect(DIAR_STATE_CLASS[state]).not.toContain("red");
      expect(DIAR_STATE_CLASS[state]).not.toContain("amber");
    }
  });

  test("model indicator colours are distinct and an idle model is grey, not red", () => {
    expect(MODEL_STATE_CLASS.healthy).toContain("green");
    expect(MODEL_STATE_CLASS.idle).toContain("gray");
    expect(MODEL_STATE_CLASS.warning).toContain("amber");
    expect(MODEL_STATE_CLASS.error).toContain("red");
    expect(MODEL_STATE_CLASS.idle).not.toContain("red");
    const unique = new Set(Object.values(MODEL_STATE_CLASS));
    expect(unique.size).toBe(4);
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
