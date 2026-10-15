/**
 * Typed wrappers for the speaker identity registry, voiceprint browser and
 * diarization commands, plus the diarization events.
 *
 * Registry and voiceprint commands live in src-tauri/src/database/speaker_commands.rs;
 * `assign_live_speaker` and `finalize_online_session` in audio/recording_commands.rs;
 * the diarization commands in audio/diarization/commands.rs.
 */
import { invokeTyped, listenTyped, type EventCallback, type UnlistenFn } from './core';
import type { DiarizationProgress, DiarizationResult, LiveTranscriptBlocks } from '@/types';

// --- Speaker registry ----------------------------------------------------------

/** Global registry speaker (`database::models::Speaker`). */
export interface Speaker {
  id: string;
  name: string;
  is_me: boolean;
  created_at: string;
  updated_at: string;
}

/** The registry speaker now bound to a meeting cluster. */
export interface AssignedSpeaker {
  /** Empty string for `assign_live_speaker` (no meeting row exists yet). */
  meeting_id: string;
  cluster_label: string;
  speaker_id: string;
  name: string;
}

/** The registry speaker now overridden on a single transcript block. */
export interface AssignedBlockSpeaker {
  transcript_id: string;
  speaker_id: string;
  name: string;
}

export async function listSpeakers(): Promise<Speaker[]> {
  return invokeTyped<Speaker[]>('list_speakers');
}

export interface FindOrCreateSpeakerArgs {
  name: string;
}

export async function findOrCreateSpeaker(args: FindOrCreateSpeakerArgs): Promise<Speaker> {
  return invokeTyped<Speaker, FindOrCreateSpeakerArgs>('find_or_create_speaker', args);
}

export interface RenameSpeakerArgs {
  speakerId: string;
  newName: string;
}

export async function renameSpeaker(args: RenameSpeakerArgs): Promise<boolean> {
  return invokeTyped<boolean, RenameSpeakerArgs>('rename_speaker', args);
}

/** Pass `speakerId` to bind an existing person, or `newName` to create one. */
export interface AssignSpeakerArgs {
  meetingId: string;
  clusterLabel: string;
  speakerId?: string | null;
  newName?: string | null;
}

export async function assignSpeaker(args: AssignSpeakerArgs): Promise<AssignedSpeaker> {
  return invokeTyped<AssignedSpeaker, AssignSpeakerArgs>('assign_speaker', args);
}

/** `scope: 'block'` with `startTime`/`endTime` relabels one live turn; unset/`'cluster'` binds the cluster. */
export interface AssignLiveSpeakerArgs {
  clusterLabel: string;
  speakerId?: string | null;
  newName?: string | null;
  scope?: string | null;
  startTime?: number | null;
  endTime?: number | null;
}

export async function assignLiveSpeaker(args: AssignLiveSpeakerArgs): Promise<AssignedSpeaker> {
  return invokeTyped<AssignedSpeaker, AssignLiveSpeakerArgs>('assign_live_speaker', args);
}

export interface AssignBlockSpeakerArgs {
  transcriptId: string;
  speakerId?: string | null;
  newName?: string | null;
}

export async function assignBlockSpeaker(
  args: AssignBlockSpeakerArgs,
): Promise<AssignedBlockSpeaker> {
  return invokeTyped<AssignedBlockSpeaker, AssignBlockSpeakerArgs>('assign_block_speaker', args);
}

export type ApplyBlockSpeakerToClusterArgs = AssignBlockSpeakerArgs;

export async function applyBlockSpeakerToCluster(
  args: ApplyBlockSpeakerToClusterArgs,
): Promise<AssignedSpeaker> {
  return invokeTyped<AssignedSpeaker, ApplyBlockSpeakerToClusterArgs>(
    'apply_block_speaker_to_cluster',
    args,
  );
}

export interface ConfirmBlockSpeakerArgs {
  transcriptId: string;
  scopeAll?: boolean | null;
}

/** Resolves to the number of rows updated. */
export async function confirmBlockSpeaker(args: ConfirmBlockSpeakerArgs): Promise<number> {
  return invokeTyped<number, ConfirmBlockSpeakerArgs>('confirm_block_speaker', args);
}

/** Rust takes a single `request: SetExpectedSpeakersRequest` struct with snake_case fields. */
export interface SetExpectedSpeakersArgs {
  request: { meeting_id: string; speaker_ids: string[] };
}

export async function setExpectedSpeakers(args: SetExpectedSpeakersArgs): Promise<void> {
  return invokeTyped<void, SetExpectedSpeakersArgs>('set_expected_speakers', args);
}

export interface GetExpectedSpeakersArgs {
  meetingId: string;
}

export async function getExpectedSpeakers(args: GetExpectedSpeakersArgs): Promise<string[]> {
  return invokeTyped<string[], GetExpectedSpeakersArgs>('get_expected_speakers', args);
}

export interface ReplaceSpeakerResult {
  affected_meetings: number;
  affected_clusters: number;
  affected_transcripts: number;
}

/** `target: null` re-binds the source's clusters as anonymous instead of to another speaker. */
export interface ReplaceSpeakerArgs {
  source: string;
  target?: string | null;
}

export async function replaceSpeaker(args: ReplaceSpeakerArgs): Promise<ReplaceSpeakerResult> {
  return invokeTyped<ReplaceSpeakerResult, ReplaceSpeakerArgs>('replace_speaker', args);
}

export interface PreviewReplaceSpeakerArgs {
  source: string;
}

export async function previewReplaceSpeaker(
  args: PreviewReplaceSpeakerArgs,
): Promise<ReplaceSpeakerResult> {
  return invokeTyped<ReplaceSpeakerResult, PreviewReplaceSpeakerArgs>(
    'preview_replace_speaker',
    args,
  );
}

// --- Voiceprints -------------------------------------------------------------------

/** One voiceprint row: an enrolled prototype (`speaker_id`) or a meeting cluster cache. */
export interface VoiceprintRow {
  id: string;
  model: string;
  channel: string;
  duration_secs: number;
  speaker_id: string | null;
  meeting_id: string | null;
  cluster_label: string | null;
  audio_start_time: number | null;
  audio_end_time: number | null;
  meeting_title: string | null;
  has_audio: boolean;
  /** 0 = unverified, 1 = user-confirmed. */
  is_verified: number;
  created_at: string;
  suspect: boolean;
  own_similarity: number | null;
}

export interface SpeakerVoiceprints {
  speaker_id: string;
  speaker_name: string;
  is_me: boolean;
  prototype_count: number;
  unverified_count: number;
  suspect_count: number;
  prototypes: VoiceprintRow[];
}

export interface MeetingVoiceprints {
  meeting_id: string;
  meeting_title: string;
  unverified_count: number;
  caches: VoiceprintRow[];
}

export interface VoiceprintBrowserData {
  speakers: SpeakerVoiceprints[];
  unconfirmed: MeetingVoiceprints[];
}

export interface ListVoiceprintsArgs {
  speakerId?: string | null;
  unconfirmedOnly?: boolean | null;
  limit?: number | null;
  offset?: number | null;
}

export async function listVoiceprints(args: ListVoiceprintsArgs): Promise<VoiceprintBrowserData> {
  return invokeTyped<VoiceprintBrowserData, ListVoiceprintsArgs>('list_voiceprints', args);
}

export interface StorageStats {
  registry_count: number;
  prototype_count: number;
  cache_count: number;
  total_bytes: number;
  audio_bytes: number;
  clip_count: number;
}

export async function speakerStorageStats(): Promise<StorageStats> {
  return invokeTyped<StorageStats>('speaker_storage_stats');
}

export interface RejectVoiceprintArgs {
  id: string;
  permanent?: boolean | null;
}

export interface RejectVoiceprintResult {
  speaker_id: string | null;
  remaining_prototypes: number;
}

export async function rejectVoiceprint(
  args: RejectVoiceprintArgs,
): Promise<RejectVoiceprintResult> {
  return invokeTyped<RejectVoiceprintResult, RejectVoiceprintArgs>('reject_voiceprint', args);
}

export interface ReconfirmVoiceprintArgs {
  id: string;
  speakerId: string;
}

/** Resolves to the row's cosine to the target's other prototypes, or null when it holds fewer than two. */
export async function reconfirmVoiceprint(args: ReconfirmVoiceprintArgs): Promise<number | null> {
  return invokeTyped<number | null, ReconfirmVoiceprintArgs>('reconfirm_voiceprint', args);
}

export interface VerifyVoiceprintArgs {
  id: string;
}

export async function verifyVoiceprint(args: VerifyVoiceprintArgs): Promise<boolean> {
  return invokeTyped<boolean, VerifyVoiceprintArgs>('verify_voiceprint', args);
}

export interface VerifySpeakerArgs {
  speakerId: string;
}

/** Resolves to the number of prototypes marked verified. */
export async function verifySpeaker(args: VerifySpeakerArgs): Promise<number> {
  return invokeTyped<number, VerifySpeakerArgs>('verify_speaker', args);
}

export interface VerifyMeetingCachesArgs {
  meetingId: string;
}

/** Resolves to the number of caches marked verified. */
export async function verifyMeetingCaches(args: VerifyMeetingCachesArgs): Promise<number> {
  return invokeTyped<number, VerifyMeetingCachesArgs>('verify_meeting_caches', args);
}

export interface GetVoiceprintAudioArgs {
  id: string;
}

/** Resolves to a temp file path allowed in the asset scope, or null for rows with no stored clip. */
export async function getVoiceprintAudio(args: GetVoiceprintAudioArgs): Promise<string | null> {
  return invokeTyped<string | null, GetVoiceprintAudioArgs>('get_voiceprint_audio', args);
}

export interface ClearAllVoiceprintsResult {
  deleted_prototypes: number;
  deleted_caches: number;
  total_deleted: number;
}

export async function clearAllVoiceprints(): Promise<ClearAllVoiceprintsResult> {
  return invokeTyped<ClearAllVoiceprintsResult>('clear_all_voiceprints');
}

export interface PurgeUnconfirmedCachesResult {
  deleted_caches: number;
  deleted_embedding_bytes: number;
  deleted_clip_count: number;
  deleted_clip_bytes: number;
}

export async function purgeUnconfirmedCaches(): Promise<PurgeUnconfirmedCachesResult> {
  return invokeTyped<PurgeUnconfirmedCachesResult>('purge_unconfirmed_caches');
}

// --- Diarization -------------------------------------------------------------------

/**
 * Keys are sent as the call site sends them today. NOTE: `max_speakers` is
 * snake_case, so Tauri (which expects `maxSpeakers`) ignores it and Rust
 * receives `None`.
 */
export interface StartDiarizationArgs {
  meetingId: string;
  max_speakers?: number;
}

export async function startDiarization(args: StartDiarizationArgs): Promise<DiarizationResult> {
  return invokeTyped<DiarizationResult, StartDiarizationArgs>('start_diarization', args);
}

export interface DiarizationModelStatus {
  segmentation_ready: boolean;
  embedding_ready: boolean;
  ready: boolean;
}

export async function checkDiarizationModels(): Promise<DiarizationModelStatus> {
  return invokeTyped<DiarizationModelStatus>('check_diarization_models');
}

export interface GetDiarizationStatusArgs {
  meetingId: string;
}

export interface DiarizationStatus {
  meeting_id: string;
  diarization_status: string | null;
  speaker_names: string | null;
}

/** Rust returns an untyped `serde_json::Value`; this is the shape it builds. */
export async function getDiarizationStatus(
  args: GetDiarizationStatusArgs,
): Promise<DiarizationStatus> {
  return invokeTyped<DiarizationStatus, GetDiarizationStatusArgs>('get_diarization_status', args);
}

export interface UpdateSpeakerLabelCommandArgs {
  meetingId: string;
  speaker: string;
  label: string;
}

export async function updateSpeakerLabelCommand(
  args: UpdateSpeakerLabelCommandArgs,
): Promise<boolean> {
  return invokeTyped<boolean, UpdateSpeakerLabelCommandArgs>('update_speaker_label_command', args);
}

export interface RematchMeetingSpeakersArgs {
  meetingId: string;
}

export interface RematchMeetingSpeakersResult {
  meeting_id: string;
  matched: number;
}

/** Rust returns an untyped `serde_json::Value`; this is the shape it builds. */
export async function rematchMeetingSpeakers(
  args: RematchMeetingSpeakersArgs,
): Promise<RematchMeetingSpeakersResult> {
  return invokeTyped<RematchMeetingSpeakersResult, RematchMeetingSpeakersArgs>(
    'rematch_meeting_speakers',
    args,
  );
}

export interface FinalizeOnlineSessionArgs {
  meetingId: string;
}

export interface FinalizeOnlineSessionResult {
  meeting_id: string;
  live_bindings: number;
  enrolled: number;
}

/** Rust returns an untyped `serde_json::Value`; this is the shape it builds. */
export async function finalizeOnlineSession(
  args: FinalizeOnlineSessionArgs,
): Promise<FinalizeOnlineSessionResult> {
  return invokeTyped<FinalizeOnlineSessionResult, FinalizeOnlineSessionArgs>(
    'finalize_online_session',
    args,
  );
}

/** `null`/omitted leaves the backend default for that setting. */
export interface SetDiarizationClusteringSettingsArgs {
  clusterThreshold?: number | null;
  clusterCeiling?: number | null;
  gapMergeSecs?: number | null;
  /** "vbx" | "nmesc" | "ahc"; anything else rejects. */
  clusterer?: string | null;
  finalReclusterEnabled?: boolean | null;
  finalRelabelAll?: boolean | null;
}

export async function setDiarizationClusteringSettings(
  args: SetDiarizationClusteringSettingsArgs,
): Promise<void> {
  return invokeTyped<void, SetDiarizationClusteringSettingsArgs>(
    'set_diarization_clustering_settings',
    args,
  );
}

// --- Events ----------------------------------------------------------------------

export function listenDiarizationProgress(
  handler: EventCallback<DiarizationProgress>,
): Promise<UnlistenFn> {
  return listenTyped<DiarizationProgress>('diarization-progress', handler);
}

/** A stable live speaker turn (Fast-mode diarization). */
export interface SpeakerTurn {
  start_time: number;
  end_time: number;
  speaker: string;
  source_device: string;
  display_name?: string;
  /** 'user' | 'auto' */
  matched_by?: string;
  /** Cosine similarity 0..1 for auto matches. */
  match_score?: number;
}

export function listenOnlineSpeakerTurn(handler: EventCallback<SpeakerTurn>): Promise<UnlistenFn> {
  return listenTyped<SpeakerTurn>('online-speaker-turn', handler);
}

export function listenLiveTranscriptBlocks(
  handler: EventCallback<LiveTranscriptBlocks>,
): Promise<UnlistenFn> {
  return listenTyped<LiveTranscriptBlocks>('live-transcript-blocks', handler);
}
