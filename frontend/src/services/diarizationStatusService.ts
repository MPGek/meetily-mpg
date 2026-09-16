/**
 * Live recording telemetry service.
 *
 * Read-only snapshot backing the two per-channel status lines and their
 * tooltip (online-diarization-telemetry): diarization counters, the pipeline
 * buffer fills that gate pipeline operations, and the activity of every model
 * in use. Sampled on the existing recording interval; the backend never emits
 * an event per audio chunk.
 */

import { invoke } from '@tauri-apps/api/core';

export type DiarChannelName = 'microphone' | 'system';

/** Display state resolved by the backend for one channel. */
export type DiarChannelState =
  | 'unavailable'
  | 'inactive'
  | 'deferred'
  | 'accumulating'
  | 'healthy'
  | 'warning'
  | 'error';

export interface DiarLastTurn {
  speaker: string;
  display_name?: string;
  /** `user` for a user binding, `auto` for a registry match, absent otherwise. */
  matched_by?: string;
  score?: number;
}

export interface DiarChannelStatus {
  channel: DiarChannelName;
  state: DiarChannelState;
  chunks: number;
  embed_ok: number;
  embed_failed: number;
  buffered: number;
  /** Audio seconds covered by the buffered embeddings. */
  buffered_secs: number;
  turns: number;
  ordered: boolean;
  last_turn?: DiarLastTurn;
}

export type DiarizationMode = 'off' | 'efficient' | 'fast';

export interface OnlineDiarizationStatus {
  /** True while an online diarization session is running. */
  active: boolean;
  mode: DiarizationMode;
  /** False when the session's engine was never constructed. */
  available: boolean;
  model_tag: string;
  embedding_dim: number;
  recognition_threshold: number;
  /** Null when no prototype store is loaded for the session. */
  prototypes: number | null;
  bindings: number | null;
  mic: DiarChannelStatus;
  sys: DiarChannelStatus;
}

/** A buffer's fill relative to the threshold that fires what it gates. */
export interface BufferFill {
  fill: number;
  threshold: number;
  /** `fill / threshold`; at or above 1 once the gated operation has fired. */
  fraction: number;
  fired: boolean;
}

/** Merged speech waiting to be sent for recognition. */
export interface PendingState {
  segments: number;
  buffered_ms: number;
  gap_trigger_ms: number;
  cap_trigger_ms: number;
}

/** Level of the last processed chunk, with how old it is. */
export interface AudioLevel {
  /** Linear RMS amplitude. */
  rms: number;
  /** Peak absolute amplitude. */
  peak: number;
  /** Milliseconds since that chunk was measured. */
  age_ms: number;
}

export interface ChannelPipelineFill {
  vad_dispatch: BufferFill;
  vad_frames: number;
  vad_speaking: boolean;
  pending: PendingState;
  mix: BufferFill;
  level: AudioLevel;
}

export interface PipelineStatus {
  sample_rate: number;
  mic: ChannelPipelineFill;
  sys: ChannelPipelineFill;
}

export interface VadActivity {
  identity: string;
  loaded: boolean;
  mic_frames: number;
  mic_speaking: boolean;
  sys_frames: number;
  sys_speaking: boolean;
}

export interface AsrActivity {
  engine: string | null;
  model: string | null;
  loaded: boolean;
  queued: number;
  completed: number;
  pending: number;
  last_text: string | null;
}

export interface AlignmentActivity {
  enabled: boolean;
  model_id: string | null;
  loaded: boolean;
  queued_jobs: number;
  queue_bytes: number;
  dropped: number;
  refined: number;
}

export interface DiarizationModelActivity {
  mode: DiarizationMode;
  model_tag: string;
  embedding_dim: number;
  recognition_threshold: number;
  loaded: boolean;
  prototypes: number | null;
  bindings: number | null;
}

/** Every model kind the recording relies on, with readiness and activity. */
export interface ModelsActivity {
  vad: VadActivity;
  asr: AsrActivity;
  alignment: AlignmentActivity;
  diarization: DiarizationModelActivity;
}

export interface RecordingTelemetry {
  active: boolean;
  diarization: OnlineDiarizationStatus;
  pipeline: PipelineStatus;
  models: ModelsActivity;
}

/** Read the current snapshot. Resolves to the inactive shape when idle. */
export async function fetchRecordingTelemetry(): Promise<RecordingTelemetry> {
  return invoke<RecordingTelemetry>('get_recording_telemetry');
}
