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
   * @returns Promise<void>
   */
  async startRecordingWithDevices(
    micDeviceName: string | null,
    systemDeviceName: string | null,
    meetingName: string,
    diarizationMode: string = "off",
    maxSpeakers: number | null = null
  ): Promise<void> {
    return invoke('start_recording_with_devices_and_meeting', {
      micDeviceName: micDeviceName,
      systemDeviceName: systemDeviceName,
      meetingName: meetingName,
      diarizationMode: diarizationMode,
      maxSpeakers: maxSpeakers,
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
}

// Export singleton instance
export const recordingService = new RecordingService();
