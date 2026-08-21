/**
 * Recording Service
 *
 * Handles all recording lifecycle Tauri backend calls and events.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke/listen calls.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

export interface RecordingState {
  is_recording: boolean;
  is_paused: boolean;
  is_active: boolean;
  recording_duration: number | null;
  active_duration: number | null;
}

export interface SpeakerAssignment {
  sequence_id: number;
  speaker: string;
}

export interface SpeakerTurn {
  start_time: number;
  end_time: number;
  speaker: string;
  source_device: string;
  display_name?: string;
  matched_by?: string; // 'user' | 'auto'
  match_score?: number; // cosine similarity 0..1 for auto matches
}

export interface RecordingStoppedPayload {
  message: string;
  folder_path?: string;
  meeting_name?: string;
  online_diarization_used?: boolean;
  speaker_assignments?: SpeakerAssignment[];
}

export interface DiarizationProgressPayload {
  meeting_id: string;
  status: string;
  progress: number;
  message: string;
}

export interface DiarizationResultPayload {
  meeting_id: string;
  segments_labeled: number;
  speakers_found: number;
}

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
    return invoke<boolean>('is_recording');
  }

  /**
   * Get comprehensive recording state (includes durations)
   * @returns Promise with full recording state
   */
  async getRecordingState(): Promise<RecordingState> {
    return invoke<RecordingState>('get_recording_state');
  }

  /**
   * Get current meeting name
   * @returns Promise<string | null>
   */
  async getRecordingMeetingName(): Promise<string | null> {
    return invoke<string | null>('get_recording_meeting_name');
  }

  /**
   * Start recording (no device configuration)
   * @returns Promise<void>
   */
  async startRecording(): Promise<void> {
    return invoke('start_recording');
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
    return invoke('start_recording_with_devices_and_meeting', {
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
    return invoke('stop_recording', {
      args: { save_path: savePath }
    });
  }

  /**
   * Pause active recording
   * @returns Promise<void>
   */
  async pauseRecording(): Promise<void> {
    return invoke('pause_recording');
  }

  /**
   * Resume paused recording
   * @returns Promise<void>
   */
  async resumeRecording(): Promise<void> {
    return invoke('resume_recording');
  }

  // Event Listeners

  /**
   * Listen for recording-started event
   * @param callback - Function to call when recording starts
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingStarted(callback: () => void): Promise<UnlistenFn> {
    return listen('recording-started', callback);
  }

  /**
   * Listen for recording-stopped event (with metadata)
   * @param callback - Function to call when recording stops
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingStopped(callback: (payload: RecordingStoppedPayload) => void): Promise<UnlistenFn> {
    return listen<RecordingStoppedPayload>('recording-stopped', (event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for recording-paused event
   * @param callback - Function to call when recording is paused
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingPaused(callback: () => void): Promise<UnlistenFn> {
    return listen('recording-paused', callback);
  }

  /**
   * Listen for recording-resumed event
   * @param callback - Function to call when recording resumes
   * @returns Promise that resolves to unlisten function
   */
  async onRecordingResumed(callback: () => void): Promise<UnlistenFn> {
    return listen('recording-resumed', callback);
  }

  /**
   * Listen for chunk-drop-warning event (audio buffer overflow)
   * @param callback - Function to call when chunks are dropped
   * @returns Promise that resolves to unlisten function
   */
  async onChunkDropWarning(callback: (warning: string) => void): Promise<UnlistenFn> {
    return listen<string>('chunk-drop-warning', (event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for speech-detected event (VAD)
   * @param callback - Function to call when speech is detected
   * @returns Promise that resolves to unlisten function
   */
  async onSpeechDetected(callback: () => void): Promise<UnlistenFn> {
    return listen('speech-detected', callback);
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
    return invoke<DiarizationResultPayload>('start_diarization', {
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
    return invoke('get_diarization_status', {
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
    return invoke<boolean>('update_speaker_label_command', {
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
    return listen<DiarizationProgressPayload>('diarization-progress', (event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for live online-speaker-turn events (Fast-mode diarization)
   * @param callback - Function to call when a stable speaker turn is emitted during recording
   * @returns Unlisten function
   */
  async onSpeakerTurn(callback: (turn: SpeakerTurn) => void): Promise<UnlistenFn> {
    return listen<SpeakerTurn>('online-speaker-turn', (event) => {
      callback(event.payload);
    });
  }

  /**
   * Check whether the diarization ONNX models are present on disk
   */
  async checkDiarizationModels(): Promise<{ segmentation_ready: boolean; embedding_ready: boolean }> {
    return invoke('check_diarization_models');
  }

  /**
   * Download the diarization ONNX models to the app data directory
   */
  async downloadDiarizationModels(): Promise<void> {
    return invoke('download_diarization_models');
  }

  /**
   * Listen for diarization model download progress events
   */
  async onDiarizationModelDownloadProgress(
    callback: (progress: number, message: string) => void
  ): Promise<UnlistenFn> {
    return listen<{ progress: number; message: string }>('diarization-model-download-progress', (event) => {
      callback(event.payload.progress, event.payload.message);
    });
  }

  /**
   * Listen for diarization model download completion
   */
  async onDiarizationModelDownloadComplete(callback: () => void): Promise<UnlistenFn> {
    return listen('diarization-model-download-complete', callback);
  }

  /**
   * Listen for diarization model download errors
   */
  async onDiarizationModelDownloadError(
    callback: (error: string) => void
  ): Promise<UnlistenFn> {
    return listen<{ error: string }>('diarization-model-download-error', (event) => {
      callback(event.payload.error);
    });
  }

  // ===== Speaker Identity Registry =====

  /**
   * List all registry speakers (for editor dropdowns).
   */
  async listSpeakers(): Promise<Array<{ id: string; name: string; is_me: boolean }>> {
    return invoke('list_speakers');
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
    return invoke('assign_speaker', {
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
    return invoke('rename_speaker', { speakerId, newName });
  }

  /**
   * Replace the expected-speaker allowlist for a meeting.
   * Empty list = recognition matches all registry speakers.
   */
  async setExpectedSpeakers(meetingId: string, speakerIds: string[]): Promise<void> {
    return invoke('set_expected_speakers', {
      request: { meeting_id: meetingId, speaker_ids: speakerIds },
    });
  }

  /**
   * Read the expected-speaker ids for a meeting.
   */
  async getExpectedSpeakers(meetingId: string): Promise<string[]> {
    return invoke('get_expected_speakers', { meetingId });
  }

  /**
   * Re-run speaker recognition from cached centroids (no audio re-processing).
   * User bindings are preserved.
   */
  async rematchMeetingSpeakers(meetingId: string): Promise<{ meeting_id: string; matched: number }> {
    return invoke('rematch_meeting_speakers', { meetingId });
  }

  /**
   * Finalize an online diarization session after the meeting row exists.
   * Persists cluster caches, enrolls embeddings, persists expected speakers.
   */
  async finalizeOnlineSession(meetingId: string): Promise<{ meeting_id: string; live_bindings: number; enrolled: number }> {
    return invoke('finalize_online_session', { meetingId });
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
    return invoke('speaker_storage_stats');
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
    return invoke('assign_block_speaker', {
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
    return invoke('apply_block_speaker_to_cluster', {
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
    return invoke('assign_live_speaker', {
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
    return invoke('assign_live_speaker', {
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
    return invoke('confirm_block_speaker', {
      transcriptId,
      scopeAll: scopeAll ?? false,
    });
  }
}

// Export singleton instance
export const recordingService = new RecordingService();
