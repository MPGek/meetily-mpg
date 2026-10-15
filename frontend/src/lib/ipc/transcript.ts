/**
 * Typed wrappers for transcript history and storage, transcription readiness,
 * retranscription (Enhance) and audio import commands, plus their events.
 *
 * `get_transcription_status` is the stub wrapper in src-tauri/src/lib.rs;
 * the meeting transcript commands live in api/api.rs and the rest under
 * src-tauri/src/audio/.
 */
import { invokeTyped, listenTyped, type EventCallback, type UnlistenFn } from './core';
import type { PaginatedTranscriptsResponse, Transcript, TranscriptToken, TranscriptUpdate } from '@/types';

// --- Live transcript ---------------------------------------------------------------

/** A segment of the active recording (`recording_saver::TranscriptSegment`). */
export interface TranscriptHistorySegment {
  id: string;
  text: string;
  audio_start_time: number;
  audio_end_time: number;
  duration: number;
  /** Formatted for display, e.g. "[02:15]". */
  display_time: string;
  confidence: number;
  sequence_id: number;
  source_device: string;
  tokens?: TranscriptToken[];
}

/** Resolves to an empty list when no recording is active. */
export async function getTranscriptHistory(): Promise<TranscriptHistorySegment[]> {
  return invokeTyped<TranscriptHistorySegment[]>('get_transcript_history');
}

export interface TranscriptionStatus {
  chunks_in_queue: number;
  is_processing: boolean;
  last_activity_ms: number;
}

/** Stub in lib.rs: always reports an idle, empty queue. */
export async function getTranscriptionStatus(): Promise<TranscriptionStatus> {
  return invokeTyped<TranscriptionStatus>('get_transcription_status');
}

export interface TranscriptionModelStatus {
  ready: boolean;
  provider: string;
  downloading: boolean;
}

export async function checkActiveTranscriptionModelReady(): Promise<TranscriptionModelStatus> {
  return invokeTyped<TranscriptionModelStatus>('check_active_transcription_model_ready');
}

// --- Meeting transcripts -----------------------------------------------------------

export interface GetMeetingTranscriptsArgs {
  meetingId: string;
  limit: number;
  offset: number;
}

export async function getMeetingTranscripts(
  args: GetMeetingTranscriptsArgs,
): Promise<PaginatedTranscriptsResponse> {
  return invokeTyped<PaginatedTranscriptsResponse, GetMeetingTranscriptsArgs>(
    'api_get_meeting_transcripts',
    args,
  );
}

export interface SaveTranscriptArgs {
  meetingTitle: string;
  transcripts: Transcript[];
  folderPath: string | null;
  authToken?: string | null;
}

export interface SaveMeetingResponse {
  status: string;
  message: string;
  meeting_id: string;
  /** Pending recording tags that could not be linked to the new meeting. */
  tag_warnings: string[];
}

/** Rust returns an untyped `serde_json::Value`; this is the shape it builds. */
export async function saveTranscript(args: SaveTranscriptArgs): Promise<SaveMeetingResponse> {
  return invokeTyped<SaveMeetingResponse, SaveTranscriptArgs>('api_save_transcript', args);
}

// --- Retranscription ---------------------------------------------------------------

export interface StartRetranscriptionCommandArgs {
  meetingId: string;
  meetingFolderPath: string;
  language?: string | null;
  model?: string | null;
  provider?: string | null;
}

export interface RetranscriptionStarted {
  meeting_id: string;
  message: string;
}

/** Resolves once the background run is spawned; outcome arrives via the retranscription events. */
export async function startRetranscriptionCommand(
  args: StartRetranscriptionCommandArgs,
): Promise<RetranscriptionStarted> {
  return invokeTyped<RetranscriptionStarted, StartRetranscriptionCommandArgs>(
    'start_retranscription_command',
    args,
  );
}

/** Rejects when no retranscription is in progress. */
export async function cancelRetranscriptionCommand(): Promise<void> {
  return invokeTyped<void>('cancel_retranscription_command');
}

// --- Audio import ------------------------------------------------------------------

export interface AudioFileInfo {
  path: string;
  filename: string;
  duration_seconds: number;
  size_bytes: number;
  format: string;
}

/** Opens the native file dialog; resolves to null when the user cancels. */
export async function selectAndValidateAudioCommand(): Promise<AudioFileInfo | null> {
  return invokeTyped<AudioFileInfo | null>('select_and_validate_audio_command');
}

export interface ValidateAudioFileCommandArgs {
  path: string;
}

export async function validateAudioFileCommand(
  args: ValidateAudioFileCommandArgs,
): Promise<AudioFileInfo> {
  return invokeTyped<AudioFileInfo, ValidateAudioFileCommandArgs>(
    'validate_audio_file_command',
    args,
  );
}

export interface StartImportAudioCommandArgs {
  sourcePath: string;
  title: string;
  language?: string | null;
  model?: string | null;
  provider?: string | null;
}

export interface ImportStarted {
  message: string;
}

/** Resolves once the background import is spawned; outcome arrives via the import events. */
export async function startImportAudioCommand(
  args: StartImportAudioCommandArgs,
): Promise<ImportStarted> {
  return invokeTyped<ImportStarted, StartImportAudioCommandArgs>(
    'start_import_audio_command',
    args,
  );
}

/** Rejects when no import is in progress. */
export async function cancelImportCommand(): Promise<void> {
  return invokeTyped<void>('cancel_import_command');
}

// --- Transcription events ----------------------------------------------------------

export function listenTranscriptUpdate(
  handler: EventCallback<TranscriptUpdate>,
): Promise<UnlistenFn> {
  return listenTyped<TranscriptUpdate>('transcript-update', handler);
}

/** No Rust code emits `transcription-complete` today; the payload is unspecified. */
export function listenTranscriptionComplete(handler: EventCallback<unknown>): Promise<UnlistenFn> {
  return listenTyped<unknown>('transcription-complete', handler);
}

/** Legacy string error. No Rust code emits `transcript-error` today. */
export function listenTranscriptError(handler: EventCallback<string>): Promise<UnlistenFn> {
  return listenTyped<string>('transcript-error', handler);
}

export interface TranscriptionErrorPayload {
  error: string;
  userMessage: string;
  /** True when the user can fix it (e.g. by picking a model). */
  actionable: boolean;
}

export function listenTranscriptionError(
  handler: EventCallback<TranscriptionErrorPayload>,
): Promise<UnlistenFn> {
  return listenTyped<TranscriptionErrorPayload>('transcription-error', handler);
}

// --- Retranscription events --------------------------------------------------------

export interface RetranscriptionProgress {
  meeting_id: string;
  /** "decoding" | "transcribing" | "saving" */
  stage: string;
  progress_percentage: number;
  message: string;
}

export interface RetranscriptionResult {
  meeting_id: string;
  segments_count: number;
  duration_seconds: number;
  language: string | null;
}

export interface RetranscriptionError {
  meeting_id: string;
  error: string;
}

export function listenRetranscriptionProgress(
  handler: EventCallback<RetranscriptionProgress>,
): Promise<UnlistenFn> {
  return listenTyped<RetranscriptionProgress>('retranscription-progress', handler);
}

export function listenRetranscriptionComplete(
  handler: EventCallback<RetranscriptionResult>,
): Promise<UnlistenFn> {
  return listenTyped<RetranscriptionResult>('retranscription-complete', handler);
}

export function listenRetranscriptionError(
  handler: EventCallback<RetranscriptionError>,
): Promise<UnlistenFn> {
  return listenTyped<RetranscriptionError>('retranscription-error', handler);
}

// --- Import events -----------------------------------------------------------------

export interface ImportProgress {
  /** "copying" | "decoding" | "vad" | "transcribing" | "saving" */
  stage: string;
  progress_percentage: number;
  message: string;
}

export interface ImportResult {
  meeting_id: string;
  title: string;
  segments_count: number;
  duration_seconds: number;
}

export interface ImportError {
  error: string;
}

export function listenImportProgress(handler: EventCallback<ImportProgress>): Promise<UnlistenFn> {
  return listenTyped<ImportProgress>('import-progress', handler);
}

export function listenImportComplete(handler: EventCallback<ImportResult>): Promise<UnlistenFn> {
  return listenTyped<ImportResult>('import-complete', handler);
}

export function listenImportError(handler: EventCallback<ImportError>): Promise<UnlistenFn> {
  return listenTyped<ImportError>('import-error', handler);
}
