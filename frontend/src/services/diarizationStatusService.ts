/**
 * Live recording telemetry service.
 *
 * Read-only snapshot backing the two per-channel status lines and their
 * tooltip (online-diarization-telemetry): diarization counters, the pipeline
 * buffer fills that gate pipeline operations, and the activity of every model
 * in use. Sampled on the existing recording interval; the backend never emits
 * an event per audio chunk.
 */

import { getRecordingTelemetry, type RecordingTelemetry } from '@/lib/ipc/recording';

export type {
  AlignmentActivity,
  AsrActivity,
  AudioLevel,
  BufferFill,
  ChannelPipelineFill,
  DiarChannelName,
  DiarChannelState,
  DiarChannelStatus,
  DiarizationMode,
  DiarizationModelActivity,
  DiarLastTurn,
  ModelsActivity,
  OnlineDiarizationStatus,
  PendingState,
  PipelineStatus,
  RecordingTelemetry,
  VadActivity,
} from '@/lib/ipc/recording';

/** Read the current snapshot. Resolves to the inactive shape when idle. */
export async function fetchRecordingTelemetry(): Promise<RecordingTelemetry> {
  return getRecordingTelemetry();
}
