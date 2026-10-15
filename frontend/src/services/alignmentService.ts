/**
 * Alignment Service
 *
 * Typed wrappers for the CTC word-alignment model management commands
 * (word-level-diarization-alignment). Mirrors the Parakeet model-management
 * surface: list/check report catalog + readiness, download streams progress
 * events, cancel keeps partials for resume, delete removes.
 */

import {
  cancelAlignmentDownload,
  checkAlignmentModels,
  deleteAlignmentModel,
  downloadAlignmentModel,
  listAlignmentModels,
  listenAlignmentModelDownloadCompleted,
  listenAlignmentModelDownloadFailed,
  listenAlignmentModelDownloadProgress,
  type AlignmentDownloadProgressPayload,
  type AlignmentModelInfo,
  type AlignmentModelStatus,
  type CancelDownloadOutcome,
} from '@/lib/ipc/models';
import type { UnlistenFn } from '@/lib/ipc/core';

export type {
  AlignmentDownloadProgressPayload,
  AlignmentModelInfo,
  AlignmentModelStatus,
} from '@/lib/ipc/models';

export class AlignmentService {
  /** List catalogued alignment models with their current status. */
  async listModels(): Promise<AlignmentModelInfo[]> {
    return listAlignmentModels();
  }

  /** Readiness of every catalogued model, keyed by id. */
  async checkModels(): Promise<Record<string, AlignmentModelStatus>> {
    return checkAlignmentModels();
  }

  /** Download a model. Resolves on completion; watch onDownloadProgress. */
  async downloadModel(modelId: string): Promise<void> {
    return downloadAlignmentModel({ modelId });
  }

  /**
   * Cancel an in-flight download; partial files are kept for resume.
   * `pending` means cleanup is still running: a `status: 'cancelled'` progress
   * event follows when it ends.
   */
  async cancelDownload(modelId: string): Promise<CancelDownloadOutcome> {
    return cancelAlignmentDownload({ modelId });
  }

  /** Delete a downloaded model. */
  async deleteModel(modelId: string): Promise<void> {
    return deleteAlignmentModel({ modelId });
  }

  async onDownloadProgress(
    callback: (payload: AlignmentDownloadProgressPayload) => void
  ): Promise<UnlistenFn> {
    return listenAlignmentModelDownloadProgress((event) => callback(event.payload));
  }

  async onDownloadCompleted(callback: (modelId: string) => void): Promise<UnlistenFn> {
    return listenAlignmentModelDownloadCompleted((event) => callback(event.payload.modelId));
  }

  async onDownloadFailed(
    callback: (modelId: string, error: string) => void
  ): Promise<UnlistenFn> {
    return listenAlignmentModelDownloadFailed((event) =>
      callback(event.payload.modelId, event.payload.error)
    );
  }
}

export const alignmentService = new AlignmentService();
