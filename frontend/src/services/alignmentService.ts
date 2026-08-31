/**
 * Alignment Service
 *
 * Typed wrappers for the CTC word-alignment model management commands
 * (word-level-diarization-alignment). Mirrors the Parakeet model-management
 * surface: list/check report catalog + readiness, download streams progress
 * events, cancel cleans partials, delete removes.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';

export type AlignmentModelStatus =
  | { state: 'Available' }
  | { state: 'Missing' }
  | { state: 'Downloading'; detail: { progress: number } }
  | { state: 'Corrupted'; detail: { file_size: number; expected_min_size: number } };

export interface AlignmentModelInfo {
  id: string;
  name: string;
  size_mb: number;
  languages: string;
  description: string;
  path: string;
  status: AlignmentModelStatus;
}

export interface AlignmentDownloadProgressPayload {
  modelId: string;
  progress: number;
  downloaded_bytes: number;
  total_bytes: number;
  speed_mbps: number;
}

export class AlignmentService {
  /** List catalogued alignment models with their current status. */
  async listModels(): Promise<AlignmentModelInfo[]> {
    return invoke<AlignmentModelInfo[]>('list_alignment_models');
  }

  /** Readiness of every catalogued model, keyed by id. */
  async checkModels(): Promise<Record<string, AlignmentModelStatus>> {
    return invoke<Record<string, AlignmentModelStatus>>('check_alignment_models');
  }

  /** Download a model. Resolves on completion; watch onDownloadProgress. */
  async downloadModel(modelId: string): Promise<void> {
    return invoke('download_alignment_model', { modelId });
  }

  /** Cancel an in-flight download and remove partial files. */
  async cancelDownload(modelId: string): Promise<void> {
    return invoke('cancel_alignment_download', { modelId });
  }

  /** Delete a downloaded model. */
  async deleteModel(modelId: string): Promise<void> {
    return invoke('delete_alignment_model', { modelId });
  }

  async onDownloadProgress(
    callback: (payload: AlignmentDownloadProgressPayload) => void
  ): Promise<UnlistenFn> {
    return listen<AlignmentDownloadProgressPayload>(
      'alignment-model-download-progress',
      (event) => callback(event.payload)
    );
  }

  async onDownloadCompleted(callback: (modelId: string) => void): Promise<UnlistenFn> {
    return listen<{ modelId: string }>('alignment-model-download-completed', (event) =>
      callback(event.payload.modelId)
    );
  }

  async onDownloadFailed(
    callback: (modelId: string, error: string) => void
  ): Promise<UnlistenFn> {
    return listen<{ modelId: string; error: string }>(
      'alignment-model-download-failed',
      (event) => callback(event.payload.modelId, event.payload.error)
    );
  }
}

export const alignmentService = new AlignmentService();
