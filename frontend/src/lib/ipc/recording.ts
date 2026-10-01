/**
 * Typed wrappers for the recording lifecycle, audio devices and backends,
 * recording preferences, live telemetry, checkpoint recovery and permission
 * commands, plus the recording events.
 *
 * `start_recording`, `start_recording_with_devices_and_meeting`,
 * `stop_recording`, `is_recording`, `get_audio_devices`,
 * `stop_audio_level_monitoring` and `trigger_microphone_permission` are the
 * wrappers in src-tauri/src/lib.rs; the rest live under src-tauri/src/audio/.
 */
import { invokeTyped, listenTyped, type EventCallback, type UnlistenFn } from './core';

// --- Recording lifecycle -------------------------------------------------------

export interface StartRecordingArgs {
  micDeviceName?: string | null;
  systemDeviceName?: string | null;
  meetingName?: string | null;
}

export async function startRecording(args?: StartRecordingArgs): Promise<void> {
  return invokeTyped<void>('start_recording', args);
}

export interface StartRecordingWithDevicesAndMeetingArgs {
  micDeviceName?: string | null;
  systemDeviceName?: string | null;
  meetingName?: string | null;
  /** "off" | "efficient" | "fast"; anything else is treated as "off". */
  diarizationMode?: string | null;
  maxSpeakers?: number | null;
  expectedSpeakerIds?: string[] | null;
}

export async function startRecordingWithDevicesAndMeeting(
  args: StartRecordingWithDevicesAndMeetingArgs,
): Promise<void> {
  return invokeTyped<void>('start_recording_with_devices_and_meeting', args);
}

/** Rust takes a single `args: RecordingArgs` struct with a snake_case field. */
export interface StopRecordingArgs {
  args: { save_path: string };
}

/** Resolves without error when no recording is active. */
export async function stopRecording(args: StopRecordingArgs): Promise<void> {
  return invokeTyped<void>('stop_recording', args);
}

export async function pauseRecording(): Promise<void> {
  return invokeTyped<void>('pause_recording');
}

export async function resumeRecording(): Promise<void> {
  return invokeTyped<void>('resume_recording');
}

/** Never rejects on the Rust side. */
export async function isRecording(): Promise<boolean> {
  return invokeTyped<boolean>('is_recording');
}

/** Built with `serde_json::json!` in Rust; durations are seconds. */
export interface RecordingState {
  is_recording: boolean;
  is_paused: boolean;
  is_active: boolean;
  recording_duration: number | null;
  active_duration: number | null;
  total_pause_duration: number;
  current_pause_duration: number | null;
}

/** Never rejects on the Rust side. */
export async function getRecordingState(): Promise<RecordingState> {
  return invokeTyped<RecordingState>('get_recording_state');
}

export async function getRecordingMeetingName(): Promise<string | null> {
  return invokeTyped<string | null>('get_recording_meeting_name');
}

// --- Audio devices and level monitoring -------------------------------------------

export interface AudioDevice {
  name: string;
  device_type: 'Input' | 'Output';
}

export async function getAudioDevices(): Promise<AudioDevice[]> {
  return invokeTyped<AudioDevice[]>('get_audio_devices');
}

export async function stopAudioLevelMonitoring(): Promise<void> {
  return invokeTyped<void>('stop_audio_level_monitoring');
}

export interface AudioOutputInfo {
  device_name: string;
  is_bluetooth: boolean;
  sample_rate: number | null;
  device_type: string;
}

export async function getActiveAudioOutput(): Promise<AudioOutputInfo> {
  return invokeTyped<AudioOutputInfo>('get_active_audio_output');
}

// --- Audio capture backend ------------------------------------------------------

export interface BackendInfo {
  id: string;
  name: string;
  description: string;
}

export interface SetAudioBackendArgs {
  backend: string;
}

export async function setAudioBackend(args: SetAudioBackendArgs): Promise<void> {
  return invokeTyped<void>('set_audio_backend', args);
}

export async function getAudioBackendInfo(): Promise<BackendInfo[]> {
  return invokeTyped<BackendInfo[]>('get_audio_backend_info');
}

export async function getCurrentAudioBackend(): Promise<string> {
  return invokeTyped<string>('get_current_audio_backend');
}

// --- Recording preferences ------------------------------------------------------

export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
  /** Only present on macOS builds. */
  system_audio_backend?: string | null;
}

export async function getRecordingPreferences(): Promise<RecordingPreferences> {
  return invokeTyped<RecordingPreferences>('get_recording_preferences');
}

export interface SetRecordingPreferencesArgs {
  preferences: RecordingPreferences;
}

export async function setRecordingPreferences(args: SetRecordingPreferencesArgs): Promise<void> {
  return invokeTyped<void>('set_recording_preferences', args);
}

export async function getDefaultRecordingsFolderPath(): Promise<string> {
  return invokeTyped<string>('get_default_recordings_folder_path');
}

// --- Live recording telemetry ---------------------------------------------------

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
  /** Blocks queued for the diarization engine but not yet consumed. */
  pending_blocks: number;
  blocks_sent: number;
  blocks_processed: number;
  blocks_in_flight: boolean;
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
  /** Speech is currently detected on any channel (indicator blink). */
  speaking: boolean;
}

export interface AsrActivity {
  engine: string | null;
  model: string | null;
  loaded: boolean;
  queued: number;
  completed: number;
  pending: number;
  /** True while the recogniser is consuming a segment. */
  in_flight: boolean;
  /** A segment was submitted but the recogniser is not yet consuming it. */
  requested: boolean;
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
  /** True while the aligner is refining a block. */
  in_flight: boolean;
  /** A block was submitted but the aligner is not yet consuming it. */
  requested: boolean;
}

export interface DiarizationModelActivity {
  mode: DiarizationMode;
  model_tag: string;
  embedding_dim: number;
  recognition_threshold: number;
  loaded: boolean;
  prototypes: number | null;
  bindings: number | null;
  /** Blocks queued for the diarization engine but not yet consumed. */
  pending_blocks: number;
  blocks_sent: number;
  blocks_completed: number;
  /** True while the engine works on a dequeued block. */
  in_flight: boolean;
  /** Blocks were submitted but the engine is not yet consuming them. */
  requested: boolean;
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

/** Resolves to the inactive shape when no session is running. */
export async function getRecordingTelemetry(): Promise<RecordingTelemetry> {
  return invokeTyped<RecordingTelemetry>('get_recording_telemetry');
}

// --- Checkpoint recovery --------------------------------------------------------

export interface MeetingFolderArgs {
  meetingFolder: string;
}

export async function hasAudioCheckpoints(args: MeetingFolderArgs): Promise<boolean> {
  return invokeTyped<boolean>('has_audio_checkpoints', args);
}

export interface RecoverAudioFromCheckpointsArgs {
  meetingFolder: string;
  /** Required by the Rust signature but unused there. */
  sampleRate: number;
}

export interface AudioRecoveryStatus {
  status: string; // "success" | "partial" | "failed" | "none"
  chunk_count: number;
  estimated_duration_seconds: number;
  /** Rust always sends the key (null when absent); optional for locally built fallbacks. */
  audio_file_path?: string | null;
  message: string;
}

export async function recoverAudioFromCheckpoints(
  args: RecoverAudioFromCheckpointsArgs,
): Promise<AudioRecoveryStatus> {
  return invokeTyped<AudioRecoveryStatus>('recover_audio_from_checkpoints', args);
}

export async function cleanupCheckpoints(args: MeetingFolderArgs): Promise<void> {
  return invokeTyped<void>('cleanup_checkpoints', args);
}

// --- Permissions ----------------------------------------------------------------

export async function triggerMicrophonePermission(): Promise<boolean> {
  return invokeTyped<boolean>('trigger_microphone_permission');
}

/** Resolves true without prompting on non-macOS platforms. */
export async function triggerSystemAudioPermissionCommand(): Promise<boolean> {
  return invokeTyped<boolean>('trigger_system_audio_permission_command');
}

// --- Events ---------------------------------------------------------------------

export interface RecordingStartedPayload {
  message: string;
  devices: string[];
  workers: number;
}

export function listenRecordingStarted(
  handler: EventCallback<RecordingStartedPayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingStartedPayload>('recording-started', handler);
}

export interface RecordingMessagePayload {
  message: string;
}

export function listenRecordingPaused(
  handler: EventCallback<RecordingMessagePayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingMessagePayload>('recording-paused', handler);
}

export function listenRecordingResumed(
  handler: EventCallback<RecordingMessagePayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingMessagePayload>('recording-resumed', handler);
}

export interface SpeakerAssignment {
  sequence_id: number;
  speaker: string;
}

export interface RecordingStoppedPayload {
  message: string;
  /** Rust sends null (not an absent key) when no meeting folder was set. */
  folder_path?: string | null;
  meeting_name?: string | null;
  online_diarization_used?: boolean;
  /** Present only when online diarization ran. */
  speaker_assignments?: SpeakerAssignment[];
}

export function listenRecordingStopped(
  handler: EventCallback<RecordingStoppedPayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingStoppedPayload>('recording-stopped', handler);
}

/** Emitted by the tray stop path; the payload is always `true`. */
export function listenRecordingStopComplete(handler: EventCallback<boolean>): Promise<UnlistenFn> {
  return listenTyped<boolean>('recording-stop-complete', handler);
}

/** Emitted once per session, on the first detected speech. */
export function listenSpeechDetected(
  handler: EventCallback<RecordingMessagePayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingMessagePayload>('speech-detected', handler);
}

/** No Rust code emits this event today; the listeners are kept as they are. */
export function listenChunkDropWarning(handler: EventCallback<string>): Promise<UnlistenFn> {
  return listenTyped<string>('chunk-drop-warning', handler);
}

export interface RecordingAudioWarningPayload {
  saved_duration_seconds: number;
  expected_duration_seconds: number;
  failed_checkpoints: number;
}

export function listenRecordingAudioWarning(
  handler: EventCallback<RecordingAudioWarningPayload>,
): Promise<UnlistenFn> {
  return listenTyped<RecordingAudioWarningPayload>('recording-audio-warning', handler);
}

export interface AudioLevelData {
  device_name: string;
  device_type: string;
  rms_level: number;
  peak_level: number;
  is_active: boolean;
}

export interface AudioLevelUpdate {
  timestamp: number;
  levels: AudioLevelData[];
}

export function listenAudioLevels(handler: EventCallback<AudioLevelUpdate>): Promise<UnlistenFn> {
  return listenTyped<AudioLevelUpdate>('audio-levels', handler);
}
