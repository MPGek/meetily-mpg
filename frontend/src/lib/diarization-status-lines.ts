/**
 * Pure formatting for the simplified live status block
 * (online-diarization-telemetry). Kept free of React so the mapping from a
 * backend snapshot to what is shown is directly testable.
 */

import type {
  AlignmentActivity,
  AsrActivity,
  AudioLevel,
  DiarChannelState,
  DiarizationModelActivity,
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
  return Math.min(
    100,
    Math.round(((db - LEVEL_FLOOR_DB) / -LEVEL_FLOOR_DB) * 100)
  );
}

/**
 * The level meter, shown first on the line. It is not a fill-to-fire buffer, so
 * it has no threshold; it does mark a clipping peak so a hot channel is
 * distinguishable from a healthy one.
 */
export function buildLevelBar(level: AudioLevel): {
  percent: number;
  firing: boolean;
  detail: string;
} {
  const stale = level.age_ms > LEVEL_STALE_MS;
  const clipping = level.peak >= 0.99;
  const db =
    level.rms > 0 ? `${(20 * Math.log10(level.rms)).toFixed(1)} dBFS` : 'silent';

  return {
    percent: levelPercent(level),
    firing: !stale && clipping,
    detail: stale
      ? `L: no audio for ${level.age_ms}ms`
      : `L: ${db} (peak ${level.peak.toFixed(2)})${clipping ? ' - clipping' : ''}`,
  };
}

/**
 * A one channel line's text content (without its `MIC`/`SYS` prefix). Only the
 * level is carried per channel; counters are shown on the model row.
 */
export function buildChannelLine(line: DiarChannelStatusLite): string {
  if (line.state === 'unavailable') {
    return 'unavailable';
  }
  if (line.state === 'inactive') {
    return 'mono session';
  }
  return DIAR_STATE_TEXT[line.state];
}

export interface DiarChannelStatusLite {
  state: DiarChannelState;
}

/** Colour semantics of a model indicator. */
export type ModelIndicatorState = 'healthy' | 'idle' | 'warning' | 'error';

/** Blink state of a model's indicator light. */
export type ModelBlink = 'none' | 'green' | 'red';

export const MODEL_STATE_CLASS: Record<ModelIndicatorState, string> = {
  healthy: 'bg-green-500',
  idle: 'bg-gray-300',
  warning: 'bg-amber-500',
  error: 'bg-red-500',
};

/** Colour overrides while the light blinks (green = working, red = requested). */
export const MODEL_BLINK_CLASS: Record<ModelBlink, string> = {
  none: '',
  green: 'bg-green-400 animate-pulse',
  red: 'bg-red-400 animate-pulse',
};

export interface ModelIndicator {
  key: 'vad' | 'asr' | 'align' | 'diar';
  label: string;
  state: ModelIndicatorState;
  blink: ModelBlink;
  /** Blocks in queue (STT / diarization); omitted when 0. */
  pending?: number;
  title: string;
}

/**
 * One indicator per model kind in use. A model with work in flight blinks
 * green; a model with a submitted request it has not started consuming blinks
 * red. A disabled or not-downloaded model is idle (grey), never an error, and
 * nothing is healthy while it is not loaded.
 */
export function buildModelIndicators(
  vad: VadActivity,
  asr: AsrActivity,
  alignment: AlignmentActivity,
  diarization: DiarizationModelActivity,
  channelStates: DiarChannelState[]
): ModelIndicator[] {
  const vadBlink: ModelBlink = vad.loaded && vad.speaking ? 'green' : 'none';
  const vadState: ModelIndicatorState = vad.loaded ? 'healthy' : 'idle';

  const asrBlink: ModelBlink =
    !asr.loaded || asr.pending === 0
      ? 'none'
      : asr.in_flight
        ? 'green'
        : 'red';
  const asrState: ModelIndicatorState = asr.loaded ? 'healthy' : 'idle';

  const alignBlink: ModelBlink =
    !alignment.enabled || alignment.queued_jobs === 0
      ? 'none'
      : alignment.in_flight
        ? 'green'
        : 'red';
  const alignmentState: ModelIndicatorState = !alignment.enabled
    ? 'idle'
    : alignment.dropped > 0
      ? 'warning'
      : alignment.loaded
        ? 'healthy'
        : 'idle';

  let diarBlink: ModelBlink = 'none';
  let diarizationState: ModelIndicatorState;
  if (diarization.mode === 'off') {
    diarizationState = 'idle';
  } else if (channelStates.includes('error')) {
    diarizationState = 'error';
  } else if (!diarization.loaded) {
    diarizationState = 'idle';
  } else if (channelStates.includes('warning')) {
    diarizationState = 'warning';
  } else {
    diarizationState = 'healthy';
    if (diarization.pending_blocks > 0) {
      diarBlink = diarization.in_flight ? 'green' : 'red';
    }
  }

  return [
    {
      key: 'vad',
      label: 'VAD',
      state: vadState,
      blink: vadBlink,
      title: `Voice activity: ${vad.identity}${vad.speaking ? ' (processing)' : ''}`,
    },
    {
      key: 'asr',
      label: 'ASR',
      state: asrState,
      blink: asrBlink,
      pending: asr.pending > 0 ? asr.pending : undefined,
      title: `Speech recognition: ${asr.pending} pending`,
    },
    {
      key: 'align',
      label: 'ALIGN',
      state: alignmentState,
      blink: alignBlink,
      pending: alignment.queued_jobs > 0 ? alignment.queued_jobs : undefined,
      title: alignment.enabled
        ? `Word alignment: ${alignment.model_id ?? 'model'}`
        : 'Word alignment: disabled',
    },
    {
      key: 'diar',
      label: 'DIAR',
      state: diarizationState,
      blink: diarBlink,
      pending:
        diarization.pending_blocks > 0 ? diarization.pending_blocks : undefined,
      title:
        diarization.mode === 'off'
          ? 'Speaker diarization: off'
          : `Speaker diarization: ${diarization.pending_blocks} pending`,
    },
  ];
}
