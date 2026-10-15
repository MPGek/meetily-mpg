// Types for Built-in AI (Summary Models) integration
import type { BuiltInModelInfo, BuiltInModelStatus } from '@/lib/ipc/models';
export type { BuiltInModelInfo, BuiltInModelStatus } from '@/lib/ipc/models';

// Helper functions for status handling
export function isModelAvailable(status: BuiltInModelStatus): boolean {
  return status.type === 'available';
}

export function isModelDownloading(status: BuiltInModelStatus): boolean {
  return status.type === 'downloading';
}

export function isModelNotDownloaded(status: BuiltInModelStatus): boolean {
  return status.type === 'not_downloaded';
}

export function isModelCorrupted(status: BuiltInModelStatus): boolean {
  return status.type === 'corrupted';
}

export function isModelError(status: BuiltInModelStatus): boolean {
  return status.type === 'error';
}

export function getStatusColor(status: BuiltInModelStatus): string {
  switch (status.type) {
    case 'available': return 'green';
    case 'downloading': return 'blue';
    case 'not_downloaded': return 'gray';
    case 'corrupted': return 'red';
    case 'error': return 'red';
    default: return 'gray';
  }
}

export function getStatusLabel(status: BuiltInModelStatus): string {
  switch (status.type) {
    case 'available': return 'Available';
    case 'downloading': return `Downloading ${status.progress}%`;
    case 'not_downloaded': return 'Not Downloaded';
    case 'corrupted': return 'Corrupted';
    case 'error': return 'Error';
    default: return 'Unknown';
  }
}

// Tauri command wrappers for Built-in AI backend
import {
  builtinAiListModels,
  builtinAiGetModelInfo,
  builtinAiIsModelReady,
  builtinAiGetAvailableSummaryModel,
  builtinAiDownloadModel,
  builtinAiCancelDownload,
  builtinAiDeleteModel,
  builtinAiGetModelsDirectory,
} from '@/lib/ipc/models';

export class BuiltInAIAPI {
  static async listModels(): Promise<BuiltInModelInfo[]> {
    return await builtinAiListModels();
  }

  static async getModelInfo(modelName: string): Promise<BuiltInModelInfo | null> {
    return await builtinAiGetModelInfo({ modelName });
  }

  static async isModelReady(modelName: string, refresh: boolean = false): Promise<boolean> {
    return await builtinAiIsModelReady({ modelName, refresh });
  }

  static async getAvailableModel(): Promise<string | null> {
    return await builtinAiGetAvailableSummaryModel();
  }

  static async downloadModel(modelName: string): Promise<void> {
    await builtinAiDownloadModel({ modelName });
  }

  static async cancelDownload(modelName: string): Promise<void> {
    await builtinAiCancelDownload({ modelName });
  }

  static async deleteModel(modelName: string): Promise<void> {
    await builtinAiDeleteModel({ modelName });
  }

  static async getModelsDirectory(): Promise<string> {
    return await builtinAiGetModelsDirectory();
  }
}
