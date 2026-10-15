/**
 * Summary IPC: generation, status polling, cancellation, saving, templates,
 * and the per-meeting summary language (src-tauri/src/summary/commands.rs,
 * summary/template_commands.rs).
 */
import type { SummaryDataResponse } from '@/types';
import { invokeTyped } from './core';

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

/** `api_get_summary` result (Rust `summary::commands::SummaryResponse`). */
export interface SummaryStatusResponse {
  /** Lowercased process status: idle, pending, processing, completed, failed, cancelled, ... */
  status: string;
  meetingName: string | null;
  meeting_id: string;
  start: string | null;
  end: string | null;
  /** Parsed summary result JSON (Rust `serde_json::Value`). */
  data: SummaryDataResponse | null;
  error: string | null;
}

export interface ProcessTranscriptResponse {
  message: string;
  process_id: string;
}

export interface CancelSummaryResponse {
  message: string;
  meeting_id: string;
}

export interface SaveMeetingSummaryResponse {
  message: string;
}

export interface TemplateInfo {
  id: string;
  name: string;
  description: string;
}

/** Where the per-meeting language lives: metadata.json, or localStorage when the meeting has no folder. */
export type SummaryLanguageStorage = 'metadata' | 'local_fallback';

export interface MeetingSummaryLanguagePreference {
  language: string | null;
  storage: SummaryLanguageStorage;
}

export type SummaryLanguageDetectionReason =
  | 'detected'
  | 'tie'
  | 'low_confidence'
  | 'unsupported'
  | 'empty';

export interface SummaryLanguageDetectionResult {
  language: string | null;
  reason: SummaryLanguageDetectionReason;
}

// ---------------------------------------------------------------------------
// Generation and status
// ---------------------------------------------------------------------------

export interface MeetingIdArgs {
  meetingId: string;
}

export interface ProcessTranscriptArgs {
  text: string;
  /** Provider id, e.g. `ollama`. */
  model: string;
  modelName: string;
  meetingId?: string | null;
  chunkSize?: number | null;
  overlap?: number | null;
  customPrompt?: string | null;
  templateId?: string | null;
  summaryLanguage?: string | null;
}

/** Never rejects for a missing process: resolves `status: 'idle'` instead. */
export async function getSummary(args: MeetingIdArgs): Promise<SummaryStatusResponse> {
  return invokeTyped<SummaryStatusResponse>('api_get_summary', args);
}

/** Starts generation in the background and resolves at once with the process id. */
export async function processTranscript(args: ProcessTranscriptArgs): Promise<ProcessTranscriptResponse> {
  return invokeTyped<ProcessTranscriptResponse>('api_process_transcript', args);
}

export interface CancelSummaryArgs {
  meetingId: string;
  /** The run to cancel: `process_id` from `processTranscript` (equals the status `start`). */
  processId: string;
}

/** Cancels only the named run; another run of the same meeting is unaffected. */
export async function cancelSummary(args: CancelSummaryArgs): Promise<CancelSummaryResponse> {
  return invokeTyped<CancelSummaryResponse>('api_cancel_summary', args);
}

export interface SaveMeetingSummaryArgs {
  meetingId: string;
  /** Stored as-is (Rust `serde_json::Value`): `{ markdown, summary_json }` or a legacy section map. */
  summary: SummaryDataResponse;
}

export async function saveMeetingSummary(args: SaveMeetingSummaryArgs): Promise<SaveMeetingSummaryResponse> {
  return invokeTyped<SaveMeetingSummaryResponse>('api_save_meeting_summary', args);
}

export async function listTemplates(): Promise<TemplateInfo[]> {
  return invokeTyped<TemplateInfo[]>('api_list_templates');
}

// ---------------------------------------------------------------------------
// Summary language
// ---------------------------------------------------------------------------

export interface DetectTranscriptSummaryLanguageArgs {
  transcriptTexts: string[];
}

export async function detectTranscriptSummaryLanguage(
  args: DetectTranscriptSummaryLanguageArgs,
): Promise<SummaryLanguageDetectionResult> {
  return invokeTyped<SummaryLanguageDetectionResult>('api_detect_transcript_summary_language', args);
}

export async function getMeetingDetectedSummaryLanguage(
  args: MeetingIdArgs,
): Promise<MeetingSummaryLanguagePreference> {
  return invokeTyped<MeetingSummaryLanguagePreference>('api_get_meeting_detected_summary_language', args);
}

export interface SaveMeetingDetectedSummaryLanguageArgs {
  meetingId: string;
  detectedSummaryLanguage?: string | null;
}

export async function saveMeetingDetectedSummaryLanguage(
  args: SaveMeetingDetectedSummaryLanguageArgs,
): Promise<MeetingSummaryLanguagePreference> {
  return invokeTyped<MeetingSummaryLanguagePreference>('api_save_meeting_detected_summary_language', args);
}

export async function getMeetingSummaryLanguage(
  args: MeetingIdArgs,
): Promise<MeetingSummaryLanguagePreference> {
  return invokeTyped<MeetingSummaryLanguagePreference>('api_get_meeting_summary_language', args);
}

export interface SaveMeetingSummaryLanguageArgs {
  meetingId: string;
  summaryLanguage?: string | null;
}

export async function saveMeetingSummaryLanguage(
  args: SaveMeetingSummaryLanguageArgs,
): Promise<MeetingSummaryLanguagePreference> {
  return invokeTyped<MeetingSummaryLanguagePreference>('api_save_meeting_summary_language', args);
}
