/**
 * Transcript Service
 *
 * Handles all transcription-related Tauri backend calls and events.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke/listen calls.
 */

import {
  getTranscriptHistory,
  getTranscriptionStatus,
  listenTranscriptError,
  listenTranscriptionComplete,
  listenTranscriptionError,
  listenTranscriptUpdate,
  type TranscriptHistorySegment,
  type TranscriptionErrorPayload,
  type TranscriptionStatus,
} from '@/lib/ipc/transcript';
import {
  listenModelDownloadComplete,
  listenParakeetModelDownloadComplete,
} from '@/lib/ipc/models';
import type { UnlistenFn } from '@/lib/ipc/core';
import { TranscriptUpdate } from '@/types';

export type { TranscriptionErrorPayload, TranscriptionStatus } from '@/lib/ipc/transcript';
export type { ModelDownloadCompletePayload } from '@/lib/ipc/models';

/**
 * Transcript Service
 * Singleton service for managing transcription operations and transcript history
 */
export class TranscriptService {
  /**
   * Get transcript history from backend (for reload sync)
   * @returns Promise<TranscriptHistorySegment[]>
   */
  async getTranscriptHistory(): Promise<TranscriptHistorySegment[]> {
    return getTranscriptHistory();
  }

  /**
   * Get current transcription queue status
   * @returns Promise with transcription status
   */
  async getTranscriptionStatus(): Promise<TranscriptionStatus> {
    return getTranscriptionStatus();
  }

  // Event Listeners

  /**
   * Listen for real-time transcript updates
   * @param callback - Function to call when new transcript segment arrives
   * @returns Promise that resolves to unlisten function
   */
  async onTranscriptUpdate(callback: (update: TranscriptUpdate) => void): Promise<UnlistenFn> {
    return listenTranscriptUpdate((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for transcription-complete event
   * @param callback - Function to call when transcription processing is complete
   * @returns Promise that resolves to unlisten function
   */
  async onTranscriptionComplete(callback: () => void): Promise<UnlistenFn> {
    return listenTranscriptionComplete(callback);
  }

  /**
   * Listen for transcription-error event (structured errors)
   * @param callback - Function to call when transcription error occurs
   * @returns Promise that resolves to unlisten function
   */
  async onTranscriptionError(callback: (error: TranscriptionErrorPayload) => void): Promise<UnlistenFn> {
    return listenTranscriptionError((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for transcript-error event (legacy error format)
   * @param callback - Function to call when transcript error occurs
   * @returns Promise that resolves to unlisten function
   */
  async onTranscriptError(callback: (error: string) => void): Promise<UnlistenFn> {
    return listenTranscriptError((event) => {
      callback(event.payload);
    });
  }

  /**
   * Listen for Whisper model download complete event
   * @param callback - Function to call when Whisper model download completes
   * @returns Promise that resolves to unlisten function
   */
  async onModelDownloadComplete(callback: (modelName: string) => void): Promise<UnlistenFn> {
    return listenModelDownloadComplete((event) => {
      callback(event.payload.modelName);
    });
  }

  /**
   * Listen for Parakeet model download complete event
   * @param callback - Function to call when Parakeet model download completes
   * @returns Promise that resolves to unlisten function
   */
  async onParakeetModelDownloadComplete(callback: (modelName: string) => void): Promise<UnlistenFn> {
    return listenParakeetModelDownloadComplete((event) => {
      callback(event.payload.modelName);
    });
  }
}

// Export singleton instance
export const transcriptService = new TranscriptService();
