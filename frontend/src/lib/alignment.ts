/**
 * Word-alignment settings (word-level-diarization-alignment 6.1).
 *
 * `wordAlignmentEnabled` (default ON) and `alignmentModelId` are persisted in
 * the browser settings store (localStorage, like the diarization settings) and
 * mirrored to the Rust backend via `set_word_alignment_settings` so the live
 * worker, stop-time repair, and offline repair all read the same values.
 */

import { invoke } from '@tauri-apps/api/core';

export const ALIGNMENT_STORAGE_KEYS = {
  enabled: 'wordAlignmentEnabled',
  modelId: 'alignmentModelId',
} as const;

export const DEFAULT_ALIGNMENT_MODEL_ID = 'wav2vec2-xlsr-56';

export interface WordAlignmentSettings {
  enabled: boolean;
  modelId: string;
}

function loadBoolean(key: string, defaultValue: boolean): boolean {
  if (typeof window === 'undefined') return defaultValue;
  const raw = localStorage.getItem(key);
  return raw !== null ? raw === 'true' : defaultValue;
}

export function loadAlignmentSettings(): WordAlignmentSettings {
  const enabled = loadBoolean(ALIGNMENT_STORAGE_KEYS.enabled, true); // default on
  const modelId =
    (typeof window !== 'undefined' && localStorage.getItem(ALIGNMENT_STORAGE_KEYS.modelId)) ||
    DEFAULT_ALIGNMENT_MODEL_ID;
  return { enabled, modelId };
}

export function saveAlignmentSettings(settings: WordAlignmentSettings): void {
  if (typeof window === 'undefined') return;
  localStorage.setItem(ALIGNMENT_STORAGE_KEYS.enabled, settings.enabled.toString());
  localStorage.setItem(ALIGNMENT_STORAGE_KEYS.modelId, settings.modelId);
}

/** Push the current settings to the Rust backend (best-effort). */
export async function syncAlignmentSettingsToBackend(
  settings: WordAlignmentSettings = loadAlignmentSettings()
): Promise<void> {
  try {
    await invoke('set_word_alignment_settings', {
      enabled: settings.enabled,
      modelId: settings.modelId,
    });
  } catch (err) {
    console.error('[alignment] Failed to sync settings to backend:', err);
  }
}
