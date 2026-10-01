/**
 * Typed wrappers for meeting CRUD/search, meeting files and audio, the tag
 * dictionary, and the first-launch database import commands.
 */
import { invokeTyped } from './core';
import type { MeetingMetadata } from '@/types';
import type { MeetingTag, MeetingTagWithUsage } from '@/lib/meeting-tags';

// --- Meetings ----------------------------------------------------------------

/** One row of `api_get_meetings` (Rust `api::Meeting`). */
export interface Meeting {
  id: string;
  title: string;
  created_at: string;
  /** When the recording began; readers fall back to `created_at`. */
  started_at: string | null;
  tags: MeetingTag[];
}

/** Transcript row inside `MeetingDetails` (Rust `api::MeetingTranscript`). */
export interface MeetingTranscript {
  id: string;
  text: string;
  timestamp: string;
  audio_start_time?: number;
  audio_end_time?: number;
  duration?: number;
  source_device?: string;
  speaker?: string;
  speaker_label?: string;
  speaker_matched_by?: string;
  speaker_match_score?: number;
}

/** Result of `api_get_meeting` (Rust `api::MeetingDetails`). */
export interface MeetingDetails {
  id: string;
  title: string;
  created_at: string;
  updated_at: string;
  transcripts: MeetingTranscript[];
  diarization_status?: string;
  speaker_names?: string;
}

export interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
  timestamp: string;
}

export interface MeetingIdArgs {
  meetingId: string;
}

export async function getMeetings(): Promise<Meeting[]> {
  return invokeTyped<Meeting[]>('api_get_meetings');
}

export async function getMeeting(args: MeetingIdArgs): Promise<MeetingDetails> {
  return invokeTyped<MeetingDetails>('api_get_meeting', args);
}

export async function getMeetingMetadata(args: MeetingIdArgs): Promise<MeetingMetadata> {
  return invokeTyped<MeetingMetadata>('api_get_meeting_metadata', args);
}

/** Resolves to a `serde_json::Value` (`{ status, message }`) on success. */
export async function deleteMeeting(args: MeetingIdArgs): Promise<unknown> {
  return invokeTyped<unknown>('api_delete_meeting', args);
}

export interface SaveMeetingTitleArgs {
  meetingId: string;
  title: string;
}

/** Resolves to a `serde_json::Value` (`{ message }`) on success. */
export async function saveMeetingTitle(args: SaveMeetingTitleArgs): Promise<unknown> {
  return invokeTyped<unknown>('api_save_meeting_title', args);
}

export interface SearchTranscriptsArgs {
  query: string;
}

export async function searchTranscripts(
  args: SearchTranscriptsArgs,
): Promise<TranscriptSearchResult[]> {
  return invokeTyped<TranscriptSearchResult[]>('api_search_transcripts', args);
}

// --- Meeting files and audio -------------------------------------------------

/** Folder of the ACTIVE recording, or null when none is set up. */
export async function getMeetingFolderPath(): Promise<string | null> {
  return invokeTyped<string | null>('get_meeting_folder_path');
}

/** Null when the meeting has no folder or no audio file; rejects if the meeting is unknown. */
export async function getMeetingAudioPath(args: MeetingIdArgs): Promise<string | null> {
  return invokeTyped<string | null>('get_meeting_audio_path', args);
}

export interface PrepareAudioForPlaybackArgs {
  filePath: string;
}

/** Transcodes to a cached WAV and resolves to its path. */
export async function prepareAudioForPlayback(args: PrepareAudioForPlaybackArgs): Promise<string> {
  return invokeTyped<string>('prepare_audio_for_playback', args);
}

export async function openMeetingFolder(args: MeetingIdArgs): Promise<void> {
  return invokeTyped<void>('open_meeting_folder', args);
}

// --- Tags --------------------------------------------------------------------

export interface AssignedTag {
  tag: MeetingTag;
  /** False when the tag was already linked to the meeting. */
  linked: boolean;
}

export interface MeetingTagArgs {
  meetingId: string;
  tagId: string;
}

export interface CreateTagArgs {
  name: string;
  color?: string | null;
}

export interface CreateAndAssignTagArgs {
  meetingId: string;
  name: string;
}

export interface TagIdArgs {
  tagId: string;
}

export interface SetTagColorArgs {
  tagId: string;
  color: string;
}

export interface SetRecordingPendingTagsArgs {
  tagIds: string[];
}

export async function listTags(): Promise<MeetingTagWithUsage[]> {
  return invokeTyped<MeetingTagWithUsage[]>('list_tags');
}

/** Returns the existing tag when a case-insensitive duplicate exists. */
export async function createTag(args: CreateTagArgs): Promise<MeetingTag> {
  return invokeTyped<MeetingTag>('create_tag', args);
}

export async function assignTag(args: MeetingTagArgs): Promise<boolean> {
  return invokeTyped<boolean>('assign_tag', args);
}

export async function unassignTag(args: MeetingTagArgs): Promise<boolean> {
  return invokeTyped<boolean>('unassign_tag', args);
}

export async function createAndAssignTag(args: CreateAndAssignTagArgs): Promise<AssignedTag> {
  return invokeTyped<AssignedTag>('create_and_assign_tag', args);
}

export async function deleteTag(args: TagIdArgs): Promise<boolean> {
  return invokeTyped<boolean>('delete_tag', args);
}

export async function setTagColor(args: SetTagColorArgs): Promise<MeetingTag> {
  return invokeTyped<MeetingTag>('set_tag_color', args);
}

/** Rejects with "No active recording" when nothing is recording. */
export async function getRecordingPendingTags(): Promise<string[]> {
  return invokeTyped<string[]>('get_recording_pending_tags');
}

/** Resolves to the canonical (trimmed, deduplicated, capped) ids; rejects when nothing is recording. */
export async function setRecordingPendingTags(args: SetRecordingPendingTagsArgs): Promise<string[]> {
  return invokeTyped<string[]>('set_recording_pending_tags', args);
}

// --- Database import (first launch) -------------------------------------------

export interface DatabaseCheckResult {
  exists: boolean;
  size: number;
}

export interface CheckHomebrewDatabaseArgs {
  path: string;
}

export interface DetectLegacyDatabaseArgs {
  selectedPath: string;
}

export interface ImportAndInitializeDatabaseArgs {
  legacyDbPath: string;
}

export async function checkHomebrewDatabase(
  args: CheckHomebrewDatabaseArgs,
): Promise<DatabaseCheckResult | null> {
  return invokeTyped<DatabaseCheckResult | null>('check_homebrew_database', args);
}

export async function checkDefaultLegacyDatabase(): Promise<string | null> {
  return invokeTyped<string | null>('check_default_legacy_database');
}

export async function detectLegacyDatabase(args: DetectLegacyDatabaseArgs): Promise<string | null> {
  return invokeTyped<string | null>('detect_legacy_database', args);
}

/** Opens a native file dialog; null when the user cancels. */
export async function selectLegacyDatabasePath(): Promise<string | null> {
  return invokeTyped<string | null>('select_legacy_database_path');
}

export async function importAndInitializeDatabase(
  args: ImportAndInitializeDatabaseArgs,
): Promise<void> {
  return invokeTyped<void>('import_and_initialize_database', args);
}

export async function initializeFreshDatabase(): Promise<void> {
  return invokeTyped<void>('initialize_fresh_database');
}
