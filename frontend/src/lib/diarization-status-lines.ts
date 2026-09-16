/**
 * Pure formatting for the live status lines and their tooltip
 * (online-diarization-telemetry). Kept free of React so the mapping from a
 * backend snapshot to what is shown is directly testable.
 */

import type {
  AlignmentActivity,
  AsrActivity,
  AudioLevel,
  BufferFill,
  ChannelPipelineFill,
  DiarChannelState,
  DiarChannelStatus,
  DiarLastTurn,
  DiarizationModelActivity,
  PendingState,
  VadActivity,
} from '../services/diarizationStatusService';

/** Short label for each resolved display state. */
export const DIAR_STATE_TEXT: Record<DiarChannelState, string> = {
  unavailable: 'unavailable',
  inactive: 'mono',
  deferred: 'cluster at stop',
  accumulating: 'waiting',
  healthy: 'ok',
  warning: 'ORDER BREAK',
  error: 'ENGINE OFF',
};

/** Text colour per state: error and warning are visibly distinct. */
export const DIAR_STATE_CLASS: Record<DiarChannelState, string> = {
  unavailable: 'text-gray-400',
  inactive: 'text-gray-400',
  deferred: 'text-gray-500',
  accumulating: 'text-gray-500',
  healthy: 'text-gray-600',
  warning: 'text-amber-600 font-medium',
  error: 'text-red-600 font-medium',
};

/** Long display names are cut so two lines fit beside the recording indicator. */
export const MAX_DIAR_NAME_CHARS = 14;

export function truncateDiarName(
  value: string,
  max: number = MAX_DIAR_NAME_CHARS
): string {
  return value.length > max ? `${value.slice(0, max - 1)}\u2026` : value;
}

/** `Alice auto 0.74` — display name over label, attribution source, score. */
export function formatDiarTurn(turn: DiarLastTurn): string {
  const name = truncateDiarName(turn.display_name ?? turn.speaker);
  const source =
    turn.matched_by === 'user' ? 'user' : turn.matched_by === 'auto' ? 'auto' : '-';
  const score = typeof turn.score === 'number' ? ` ${turn.score.toFixed(2)}` : '';
  return `${name} ${source}${score}`;
}

/** `50%` — how full a gated buffer is, as a whole percentage. */
export function formatPercent(fraction: number): string {
  if (!Number.isFinite(fraction) || fraction <= 0) {
    return '0%';
  }
  return `${Math.round(fraction * 100)}%`;
}

/** `12.4s` — milliseconds as seconds with one decimal. */
export function formatSeconds(ms: number): string {
  const seconds = Math.max(0, ms) / 1000;
  return `${seconds.toFixed(1)}s`;
}

/**
 * A gated buffer rendered as a bar. `percent` is clamped to the track (a buffer
 * can exceed its threshold, which must not overflow the bar); `fired` carries
 * the fact that it did, separately.
 */
export interface BufferBar {
  label: string;
  percent: number;
  fired: boolean;
  /** Fill and threshold as text, for the bar's hover title. */
  detail: string;
}

function barFromFill(label: string, fill: BufferFill, gate: string): BufferBar {
  return {
    label,
    percent: clampPercent(fill.fraction),
    fired: fill.fired,
    detail: `${label}: ${formatPercent(fill.fraction)} of ${gate}`,
  };
}

/** Pending speech has no single threshold; it flushes at its duration cap. */
function pendingBar(pending: PendingState): BufferBar {
  const fraction =
    pending.cap_trigger_ms > 0 ? pending.buffered_ms / pending.cap_trigger_ms : 0;
  return {
    label: 'p',
    percent: clampPercent(fraction),
    fired: pending.cap_trigger_ms > 0 && pending.buffered_ms >= pending.cap_trigger_ms,
    detail: `p: ${formatSeconds(pending.buffered_ms)} held, flushes on ${
      pending.gap_trigger_ms
    }ms gap or ${pending.cap_trigger_ms}ms cap`,
  };
}

function clampPercent(fraction: number): number {
  if (!Number.isFinite(fraction) || fraction <= 0) {
    return 0;
  }
  return Math.min(100, Math.round(fraction * 100));
}

/**
 * The three gated-buffer bars shown inside one channel line: the
 * voice-activity dispatch buffer, the pending speech held for recognition, and
 * the recording mix window. Each is labelled with the operation it gates.
 */
export function buildBufferBars(fill: ChannelPipelineFill): BufferBar[] {
  return [
    barFromFill('v', fill.vad_dispatch, 'the voice-activity dispatch window'),
    pendingBar(fill.pending),
    barFromFill('m', fill.mix, 'the recording mix window'),
  ];
}

/** Level below which a channel is treated as silent for the meter. */
export const LEVEL_FLOOR_DB = -60;

/** A level older than this reads as empty: nothing is arriving on the channel. */
export const LEVEL_STALE_MS = 500;

/**
 * Input level on a decibel meter, 0..100. A linear amplitude meter would look
 * dead for normal speech, so the scale is dB with a practical floor.
 */
export function levelPercent(level: AudioLevel): number {
  if (level.age_ms > LEVEL_STALE_MS) {
    return 0;
  }
  if (!Number.isFinite(level.rms) || level.rms <= 0) {
    return 0;
  }
  const db = 20 * Math.log10(level.rms);
  if (db <= LEVEL_FLOOR_DB) {
    return 0;
  }
  return Math.min(100, Math.round(((db - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB) * 100));
}

/**
 * The level meter, shown first on the line. It is not a fill-to-fire buffer, so
 * it has no threshold; it does mark a clipping peak so a hot channel is
 * distinguishable from a healthy one.
 */
export function buildLevelBar(level: AudioLevel): BufferBar {
  const stale = level.age_ms > LEVEL_STALE_MS;
  const clipping = level.peak >= 0.99;
  const db = level.rms > 0 ? `${(20 * Math.log10(level.rms)).toFixed(1)} dBFS` : 'silent';

  return {
    label: 'L',
    percent: levelPercent(level),
    fired: !stale && clipping,
    detail: stale
      ? `L: no audio for ${level.age_ms}ms`
      : `L: ${db} (peak ${level.peak.toFixed(2)})${clipping ? ' - clipping' : ''}`,
  };
}

/**
 * One channel line's text content (without its `MIC`/`SYS` prefix and without
 * the bars). Zero-valued parts are omitted so an idle channel reads as waiting,
 * not as a fault.
 */
export function buildChannelLine(
  line: DiarChannelStatus,
  fill: ChannelPipelineFill
): string {
  if (line.state === 'unavailable') {
    return 'unavailable';
  }
  if (line.state === 'inactive') {
    return 'mono session';
  }

  const parts: string[] = [`${line.chunks}c`, `${line.embed_ok}/${line.embed_failed}f`];

  if (fill.vad_speaking) {
    parts.push('sp');
  }
  if (line.buffered_secs > 0) {
    parts.push(formatSeconds(line.buffered_secs * 1000));
  }
  if (line.turns > 0) {
    parts.push(`${line.turns}t`);
  }
  if (line.last_turn) {
    parts.push(formatDiarTurn(line.last_turn));
  }
  parts.push(DIAR_STATE_TEXT[line.state]);

  return parts.join(' \u00b7 ');
}

/**
 * Global context required to read a match score: mode, model identity and
 * dimension, the recognition threshold, and prototype/binding counts.
 */
export function buildDiarContext(diar: DiarizationModelActivity): string {
  const prototypes =
    diar.prototypes === null
      ? 'prototypes unavailable'
      : `proto ${diar.prototypes} / bind ${diar.bindings ?? 0}`;

  const loaded = diar.mode === 'off' ? 'off' : diar.loaded ? 'loaded' : 'not loaded';

  return [
    `${diar.mode} mode`,
    `${diar.model_tag} ${diar.embedding_dim}d`,
    `tau ${diar.recognition_threshold.toFixed(2)}`,
    prototypes,
    loaded,
  ].join(' \u00b7 ');
}

/**
 * Legend naming what each gated buffer fires and at what threshold, so the
 * compact `v`/`p`/`m` tokens on a line are unambiguous.
 */
export function buildBufferLegend(pipeline: {
  sample_rate: number;
  mic: ChannelPipelineFill;
}): string {
  const toMs = (samples: number) =>
    pipeline.sample_rate > 0
      ? `${Math.round((samples / pipeline.sample_rate) * 1000)}ms`
      : `${samples} samples`;
  const { gap_trigger_ms, cap_trigger_ms } = pipeline.mic.pending;

  return [
    'BUF',
    `v = ${toMs(pipeline.mic.vad_dispatch.threshold)} voice-activity dispatch`,
    `p = pending speech (flush on ${gap_trigger_ms}ms gap or ${cap_trigger_ms}ms cap)`,
    `m = ${toMs(pipeline.mic.mix.threshold)} recording mix window`,
  ].join(' \u00b7 ');
}

/** Colour semantics of a model indicator. */
export type ModelIndicatorState = 'healthy' | 'idle' | 'warning' | 'error';

export const MODEL_STATE_CLASS: Record<ModelIndicatorState, string> = {
  healthy: 'bg-green-500',
  idle: 'bg-gray-300',
  warning: 'bg-amber-500',
  error: 'bg-red-500',
};

export interface ModelIndicator {
  key: 'vad' | 'asr' | 'align' | 'diar';
  label: string;
  state: ModelIndicatorState;
  title: string;
}

/**
 * One indicator per model kind in use. A disabled or not-downloaded model is
 * idle (grey), never an error, and nothing is healthy while it is not loaded.
 */
export function buildModelIndicators(
  vad: VadActivity,
  asr: AsrActivity,
  alignment: AlignmentActivity,
  diarization: DiarizationModelActivity,
  channelStates: DiarChannelState[]
): ModelIndicator[] {
  const vadState: ModelIndicatorState = vad.loaded ? 'healthy' : 'idle';

  const asrState: ModelIndicatorState = asr.loaded ? 'healthy' : 'idle';

  const alignmentState: ModelIndicatorState = !alignment.enabled
    ? 'idle'
    : alignment.dropped > 0
      ? 'warning'
      : alignment.loaded
        ? 'healthy'
        : 'idle';

  let diarizationState: ModelIndicatorState;
  if (diarization.mode === 'off') {
    diarizationState = 'idle';
  } else if (channelStates.includes('error')) {
    diarizationState = 'error';
  } else if (channelStates.includes('warning')) {
    diarizationState = 'warning';
  } else {
    diarizationState = diarization.loaded ? 'healthy' : 'idle';
  }

  return [
    {
      key: 'vad',
      label: 'VAD',
      state: vadState,
      title: `Voice activity: ${vad.identity} (${vadState})`,
    },
    {
      key: 'asr',
      label: 'ASR',
      state: asrState,
      title: `Speech recognition: ${[asr.engine, asr.model]
        .filter(Boolean)
        .join(' ') || 'idle'} (${asrState})`,
    },
    {
      key: 'align',
      label: 'ALIGN',
      state: alignmentState,
      title: alignment.enabled
        ? `Word alignment: ${alignment.model_id ?? 'model'} (${alignmentState})`
        : 'Word alignment: disabled',
    },
    {
      key: 'diar',
      label: 'DIAR',
      state: diarizationState,
      title:
        diarization.mode === 'off'
          ? 'Speaker diarization: off'
          : `Speaker diarization: ${diarization.model_tag} (${diarizationState})`,
    },
  ];
}

function asrLine(asr: AsrActivity): string {
  if (!asr.engine) {
    return 'ASR: idle';
  }
  const identity = [asr.engine, asr.model].filter(Boolean).join(' ');
  const status = asr.loaded ? 'loaded' : 'not loaded';
  const queue = `queue ${asr.completed}/${asr.queued}`;
  const parts = [`ASR ${identity} ${status}`, queue];
  if (asr.pending > 0) {
    parts.push(`${asr.pending} pending`);
  }
  if (asr.last_text) {
    parts.push(`last ${truncateDiarName(asr.last_text, 32)}`);
  }
  return parts.join(' \u00b7 ');
}

function alignmentLine(alignment: AlignmentActivity): string {
  if (!alignment.enabled) {
    return 'ALIGN: disabled';
  }
  const status = alignment.loaded ? 'loaded' : 'not loaded';
  const parts = [
    `ALIGN ${alignment.model_id ?? 'model'} ${status}`,
    `queue ${alignment.queued_jobs}`,
    `refined ${alignment.refined}`,
  ];
  if (alignment.dropped > 0) {
    parts.push(`dropped ${alignment.dropped}`);
  }
  return parts.join(' \u00b7 ');
}

/**
 * Tooltip lines. Every model kind the recording uses is named, with readiness
 * and the counters that show it is working; a disabled or unloaded model reads
 * as such, never as a failure.
 */
export function buildModelLines(
  vad: VadActivity,
  asr: AsrActivity,
  alignment: AlignmentActivity,
  diarization: DiarizationModelActivity,
  pipeline: { sample_rate: number; mic: ChannelPipelineFill }
): string[] {
  const vadStatus = vad.loaded ? 'loaded' : 'not loaded';
  const micActivity = `${vad.mic_frames}f${vad.mic_speaking ? ' sp' : ''}`;
  const sysActivity = `${vad.sys_frames}f${vad.sys_speaking ? ' sp' : ''}`;
  return [
    `VAD ${vad.identity} ${vadStatus} \u00b7 mic ${micActivity} \u00b7 sys ${sysActivity}`,
    asrLine(asr),
    alignmentLine(alignment),
    `DIAR ${buildDiarContext(diarization)}`,
    buildBufferLegend(pipeline),
  ];
}
