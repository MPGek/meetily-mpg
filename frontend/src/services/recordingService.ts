/**
 * Recording Service
 *
 * Handles all recording lifecycle Tauri backend calls and events.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke/listen calls.
 */

import {
  getRecordingMeetingName,
  getRecordingState,
  isRecording,
  listenChunkDropWarning,
  listenMicDeviceSwitched,
  listenMicRecoveryExhausted,
  listenMicSwapFailed,
  listenRecordingPaused,
  listenRecordingResumed,
  listenRecordingStarted,
  listenRecordingStopped,
  listenSpeechDetected,
  pauseRecording,
  resumeRecording,
  startRecording,
  startRecordingWithDevicesAndMeeting,
  stopRecording,
  type MicDeviceSwitchedPayload,
  type MicRecoveryExhaustedPayload,
  type MicSwapFailedPayload,
  type RecordingState,
  type RecordingStoppedPayload,
} from '@/lib/ipc/recording';
import {
  applyBlockSpeakerToCluster,
  assignBlockSpeaker,
  assignLiveSpeaker,
  assignSpeaker,
  checkDiarizationModels,
  clearAllVoiceprints,
  confirmBlockSpeaker,
  finalizeOnlineSession,
  getDiarizationStatus,
  getExpectedSpeakers,
  listenDiarizationProgress,
  listenLiveTranscriptBlocks,
  listenOnlineSpeakerTurn,
  listSpeakers,
  purgeUnconfirmedCaches,
  rematchMeetingSpeakers,
  renameSpeaker,
  setExpectedSpeakers,
  speakerStorageStats,
  startDiarization,
  updateSpeakerLabelCommand,
  type SpeakerTurn,
} from '@/lib/ipc/speakers';
import type { UnlistenFn } from '@/lib/ipc/core';
import type { DiarizationProgress, DiarizationResult, LiveTranscriptBlocks } from '@/types';

export type {
  MicDeviceSwitchedPayload,
  MicRecoveryExhaustedPayload,
  MicSwapFailedPayload,
  RecordingState,
  RecordingStoppedPayload,
  SpeakerAssignment,
} from '@/lib/ipc/recording';
export type { SpeakerTurn } from '@/lib/ipc/speakers';
export type DiarizationProgressPayload = DiarizationProgress;
export type DiarizationResultPayload = DiarizationResult;

/**
 * Recording Service
 * Singleton service for managing recording lifecycle operations
 */
export class RecordingService {
  /**
   * Check if recording is currently active
   * @returns Promise<boolean>
   */
  async isRecording(): Promise<boolean> {
    return isRecording();
  }

  /**
   * Get comprehensive recording state (includes durations)
   * @returns Promise with full recording state
   */
  async getRecordingState(): Promise<RecordingState> {
    return getRecordingState();
  }

  /**
   * Get current meeting name
   * @returns Promise<string | null>
   */
  async getRecordingMeetingName(): Promise<string | null> {
    return getRecordingMeetingName();
  }

  /**
   * Start recording (no device configuration)
   * @returns Promise<void>
   */
  async startRecording(): Promise<void> {
    return startRecording();
  }

  /**
   * Start recording with device configuration and meeting name
   * @param micDeviceName - Microphone device name (null for default)
   * @param systemDeviceName - System audio device name (null for none)
   * @param meetingName - Meeting name/title
   * @param diarizationMode - Online diarization mode: "off" | "efficient" | "fast"
   * @param maxSpeakers - Maximum number of speakers (null for auto-detect)
   * @param expectedSpeakerIds - Registry speaker IDs expected at this meeting (null/empty = match all)
   * @returns Promise<void>
   */
  async startRecordingWithDevices(
    micDeviceName: string | null,
    systemDeviceName: string | null,
    meetingName: string,
    diarizationMode: string = "off",
    maxSpeakers: number | null = null,
    expectedSpeakerIds: string[] | null = null
  ): Promise<void> {
    return startRecordingWithDevicesAndMeeting({
      micDeviceName: micDeviceName,
      systemDeviceName: systemDeviceName,
      meetingName: meetingName,
      diarizationMode: diarizationMode,
      maxSpeakers: maxSpeakers,
      expectedSpeakerIds: expectedSpeakerIds,
    });
  }

  /**
   * Stop recording and save to file
   * @param savePath - Path to save audio file
   * @returns Promise<void>
   */
  async stopRecording(savePath: string): Promise<void> {
    return stopRecording({
      args: { save_path: savePath }
    });
  }

  /**
   * Pause active recording
   * @returns Promise<void>
   */
  async pauseRecording(): Promise<void> {
    return pauseRecording();
  }

  /**
   * Resume paused recording
   * @returns Promise<void>
   */
  async resumeRecording(): Promise<void> {
    return resumeRecording();
  }

  // Event Listeners

  /**
   * Listen for recording-started event
   * @param callback - Function to call when recording starts
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingStarted(callback: () => void): Promise<UnlistenFn> {
    return listenRecordingStarted(callback);
  }

  /**
   * Listen for recording-stopped event (with metadata)
   * @param callback - Function to call when recording stops
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingStopped(callback: (payload: RecordingStoppedPayload) => void): Promise<UnlistenFn> {
    return listenRecordingStopped((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for recording-paused event
   * @param callback - Function to call when recording is paused
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingPaused(callback: () => void): Promise<UnlistenFn> {
    return listenRecordingPaused(callback);
  }

  /**
   * Listen for recording-resumed event
   * @param callback - Function to call when recording resumes
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingResumed(callback: () => void): Promise<UnlistenFn> {
    return listenRecordingResumed(callback);
  }

  /**
   * Listen for chunk-drop-warning event (audio buffer overflow)
   * @param callback - Function to call when chunks are dropped
   * @returns Promise that resolves to unlisten function
   */
  async onChunkDropWarning(callback: (warning: string) => void): Promise<UnlistenFn> {
    return listenChunkDropWarning((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for speech-detected event (VAD)
   * @param callback - Function to call when speech is detected
   * @returns Promise that resolves to unlisten function
   */
  async onSpeechDetected(callback: () => void): Promise<UnlistenFn> {
    return listenSpeechDetected(callback);
  }

  /**
   * Listen for mic-device-switched event (the recording mic changed without
   * the user picking it: mid-recording fallback or unavailable at start)
   * @param callback - Function to call with the switch details
   * @returns Promise that resolves to unlisten function
   */
  async onMicDeviceSwitched(callback: (payload: MicDeviceSwitchedPayload) => void): Promise<UnlistenFn> {
    return listenMicDeviceSwitched((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for mic-swap-failed event (a mid-recording fallback attempt failed)
   * @param callback - Function to call with the failed attempt
   * @returns Promise that resolves to unlisten function
   */
  async onMicSwapFailed(callback: (payload: MicSwapFailedPayload) => void): Promise<UnlistenFn> {
    return listenMicSwapFailed((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for mic-recovery-exhausted event (mid-recording recovery gave up)
   * @param callback - Function to call with the lost device
   * @returns Promise that resolves to unlisten function
   */
  async onMicRecoveryExhausted(callback: (payload: MicRecoveryExhaustedPayload) => void): Promise<UnlistenFn> {
    return listenMicRecoveryExhausted((event) => {
      callback(event.payload);
    });
  }

  // Diarization Methods

  /**
   * Start speaker diarization on a meeting's audio
   * @param meetingId - Meeting ID to diarize
   * @param maxSpeakers - Optional maximum number of speakers (0/null for auto-detect)
   * @returns Promise with result
   */
  async startDiarization(
    meetingId: string,
    maxSpeakers?: number
  ): Promise<DiarizationResultPayload> {
    return startDiarization({
      meetingId: meetingId,
      max_speakers: maxSpeakers ?? 0,
    });
  }

  /**
   * Get diarization status and speaker names for a meeting
   * @param meetingId - Meeting ID
   * @returns Promise with status info
   */
  async getDiarizationStatus(meetingId: string): Promise<{
    meeting_id: string;
    diarization_status: string | null;
    speaker_names: string | null;
  }> {
    return getDiarizationStatus({
      meetingId: meetingId,
    });
  }

  /**
   * Update a speaker's display label
   * @param meetingId - Meeting ID
   * @param speaker - Speaker ID (e.g., "SPEAKER_00")
   * @param label - User-friendly label (e.g., "Alice")
   */
  async updateSpeakerLabel(
    meetingId: string,
    speaker: string,
    label: string
  ): Promise<boolean> {
    return updateSpeakerLabelCommand({
      meetingId: meetingId,
      speaker: speaker,
      label: label,
    });
  }

  /**
   * Listen for diarization-progress events
   * @param callback - Function to call on progress updates
   * @returns Unlisten function
   */
  async onDiarizationProgress(
    callback: (payload: DiarizationProgressPayload) => void
  ): Promise<UnlistenFn> {
    return listenDiarizationProgress((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for live online-speaker-turn events (Fast-mode diarization)
   * @param callback - Function to call when a stable speaker turn is emitted during recording
   * @returns Unlisten function
   */
  async onSpeakerTurn(callback: (turn: SpeakerTurn) => void): Promise<UnlistenFn> {
    return listenOnlineSpeakerTurn((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for live word-level diarization display revisions
   * (live-word-level-diarization). Each payload is a new revision of one
   * transcript block's display sub-rows; the frontend keeps only the latest
   * revision per parent sequence_id.
   * @returns Unlisten function
   */
  async onLiveTranscriptBlocks(
    callback: (payload: LiveTranscriptBlocks) => void
  ): Promise<UnlistenFn> {
    return listenLiveTranscriptBlocks((event) => {
      callback(event.payload);
    });
  }

  /**
   * Check whether the bundled enhanced diarization models (segmentation-3.0 +
   * TitaNet-Large) are available. The models are bundled at build time; there
   * is no runtime download.
   */
  async checkDiarizationModels(): Promise<{ segmentation_ready: boolean; embedding_ready: boolean; ready: boolean }> {
    return checkDiarizationModels();
  }

  // ===== Speaker Identity Registry =====

  /**
   * List all registry speakers (for editor dropdowns).
   */
  async listSpeakers(): Promise<Array<{ id: string; name: string; is_me: boolean }>> {
    return listSpeakers();
  }

  /**
   * Link a meeting cluster to a registry speaker with matched_by='user'
   * and enroll the cluster's cached embeddings as that speaker's prototypes.
   */
  async assignSpeaker(
    meetingId: string,
    clusterLabel: string,
    speakerId?: string,
    newName?: string
  ): Promise<{ meeting_id: string; cluster_label: string; speaker_id: string; name: string }> {
    return assignSpeaker({
      meetingId,
      clusterLabel,
      speakerId: speakerId ?? null,
      newName: newName ?? null,
    });
  }

  /**
   * Globally rename a registry speaker. Applies to every meeting via the
   * read-time join.
   */
  async renameSpeaker(speakerId: string, newName: string): Promise<boolean> {
    return renameSpeaker({ speakerId, newName });
  }

  /**
   * Replace the expected-speaker allowlist for a meeting.
   * Empty list = recognition matches all registry speakers.
   */
  async setExpectedSpeakers(meetingId: string, speakerIds: string[]): Promise<void> {
    return setExpectedSpeakers({
      request: { meeting_id: meetingId, speaker_ids: speakerIds },
    });
  }

  /**
   * Read the expected-speaker ids for a meeting.
   */
  async getExpectedSpeakers(meetingId: string): Promise<string[]> {
    return getExpectedSpeakers({ meetingId });
  }

  /**
   * Re-run speaker recognition from cached centroids (no audio re-processing).
   * User bindings are preserved.
   */
  async rematchMeetingSpeakers(meetingId: string): Promise<{ meeting_id: string; matched: number }> {
    return rematchMeetingSpeakers({ meetingId });
  }

  /**
   * Finalize an online diarization session after the meeting row exists.
   * Persists cluster caches, enrolls embeddings, persists expected speakers.
   */
  async finalizeOnlineSession(meetingId: string): Promise<{ meeting_id: string; live_bindings: number; enrolled: number }> {
    return finalizeOnlineSession({ meetingId });
  }

  /**
   * Voiceprint storage statistics (registry speakers, prototypes, caches, bytes).
   */
  async speakerStorageStats(): Promise<{
    registry_count: number;
    prototype_count: number;
    cache_count: number;
    total_bytes: number;
  }> {
    return speakerStorageStats();
  }

  /**
   * Bulk removal of all voiceprints and cached embeddings.
   * Deletes every row from `speaker_embeddings` and clears in-memory
   * PrototypeStore. Returns counts of deleted prototypes/caches.
   */
  async clearAllVoiceprints(): Promise<{
    deleted_prototypes: number;
    deleted_caches: number;
    total_deleted: number;
  }> {
    return clearAllVoiceprints();
  }

  /**
   * Bulk removal of the unconfirmed cache layer only. Deletes every embedding
   * owned by a meeting cluster and keeps enrolled prototypes, the speaker
   * registry, cluster bindings/centroids, expected speakers, and transcript
   * overrides. Returns the deleted cache count and the reclaimed storage.
   */
  async purgeUnconfirmedCaches(): Promise<{
    deleted_caches: number;
    deleted_embedding_bytes: number;
    deleted_clip_count: number;
    deleted_clip_bytes: number;
  }> {
    return purgeUnconfirmedCaches();
  }

  /**
   * Relabel a single transcript block via a per-transcript speaker override.
   * Does NOT touch the cluster mapping or enroll embeddings.
   */
  async assignBlockSpeaker(
    transcriptId: string,
    speakerId?: string,
    newName?: string
  ): Promise<{ transcript_id: string; speaker_id: string; name: string }> {
    return assignBlockSpeaker({
      transcriptId,
      speakerId: speakerId ?? null,
      newName: newName ?? null,
    });
  }

  /**
   * Apply a speaker to all blocks of a transcript's cluster (the "apply to
   * all blocks of this speaker" editor option). Cluster-wide link + enrollment.
   */
  async applyBlockSpeakerToCluster(
    transcriptId: string,
    speakerId?: string,
    newName?: string
  ): Promise<{ meeting_id: string; cluster_label: string; speaker_id: string; name: string }> {
    return applyBlockSpeakerToCluster({
      transcriptId,
      speakerId: speakerId ?? null,
      newName: newName ?? null,
    });
  }

  /**
   * Assign a live speaker during Fast-mode recording. Updates the in-memory
   * prototype store so subsequent chunks match immediately.
   */
  async assignLiveSpeaker(
    clusterLabel: string,
    speakerId?: string,
    newName?: string
  ): Promise<{ cluster_label: string; speaker_id: string; name: string }> {
    return assignLiveSpeaker({
      clusterLabel,
      speakerId: speakerId ?? null,
      newName: newName ?? null,
      scope: null,
      startTime: null,
      endTime: null,
    });
  }

  /**
   * Relabel a single live turn (Fast mode) via a per-turn override scoped to
   * the turn's time range. Takes effect immediately and persists to the
   * matched transcript at stop. `startTime`/`endTime` are audio seconds.
   */
  async assignLiveSpeakerBlock(
    clusterLabel: string,
    startTime: number,
    speakerId?: string,
    newName?: string,
    endTime?: number
  ): Promise<{ cluster_label: string; speaker_id: string; name: string }> {
    return assignLiveSpeaker({
      clusterLabel,
      speakerId: speakerId ?? null,
      newName: newName ?? null,
      scope: 'block',
      startTime,
      endTime: endTime ?? null,
    });
  }

  /**
   * Confirm that an automatically recognized speaker is correct without
   * changing the name: marks the cluster/block as user-owned and clears the
   * auto confidence so the `(auto) xx%` suffix drops. Does NOT re-enroll
   * voiceprints. `scopeAll` confirms the whole cluster; otherwise only this
   * single block.
   */
  async confirmBlockSpeaker(
    transcriptId: string,
    scopeAll?: boolean
  ): Promise<number> {
    return confirmBlockSpeaker({
      transcriptId,
      scopeAll: scopeAll ?? false,
    });
  }
}

// Export singleton instance
export const recordingService = new RecordingService();
