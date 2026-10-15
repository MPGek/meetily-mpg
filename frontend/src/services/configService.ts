/**
 * Configuration Service
 *
 * Handles all configuration-related Tauri backend calls.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke calls.
 */

import {
  getCustomOpenaiConfig,
  getModelConfig,
  getTranscriptConfig,
  saveCustomOpenaiConfig,
  testCustomOpenaiConnection,
  type CustomOpenAIConfig,
  type ModelConfig,
  type TranscriptModelProps,
} from '@/lib/ipc/settings';
import { getRecordingPreferences, type RecordingPreferences } from '@/lib/ipc/recording';

export type { CustomOpenAIConfig, ModelConfig } from '@/lib/ipc/settings';
export type { RecordingPreferences } from '@/lib/ipc/recording';

/**
 * Configuration Service
 * Singleton service for managing app configuration
 */
export class ConfigService {
  /**
   * Get saved transcript model configuration
   * @returns Promise with { provider, model, apiKey }
   */
  async getTranscriptConfig(): Promise<TranscriptModelProps | null> {
    return getTranscriptConfig();
  }

  /**
   * Get saved summary model configuration
   * @returns Promise with { provider, model, whisperModel }
   */
  async getModelConfig(): Promise<ModelConfig | null> {
    return getModelConfig();
  }

  /**
   * Get saved audio device preferences
   * @returns Promise with { preferred_mic_device, preferred_system_device }
   */
  async getRecordingPreferences(): Promise<RecordingPreferences> {
    return getRecordingPreferences();
  }

  /**
   * Get custom OpenAI configuration
   * @returns Promise with CustomOpenAIConfig or null if not configured
   */
  async getCustomOpenAIConfig(): Promise<CustomOpenAIConfig | null> {
    return getCustomOpenaiConfig();
  }

  /**
   * Save custom OpenAI configuration
   * @param config - CustomOpenAIConfig to save
   * @returns Promise with result status
   */
  async saveCustomOpenAIConfig(config: CustomOpenAIConfig): Promise<{ status: string; message: string }> {
    return saveCustomOpenaiConfig({
      endpoint: config.endpoint,
      apiKey: config.apiKey,
      model: config.model,
      maxTokens: config.maxTokens,
      temperature: config.temperature,
      topP: config.topP,
    });
  }

  /**
   * Test custom OpenAI connection
   * @param endpoint - API endpoint URL
   * @param apiKey - Optional API key
   * @param model - Model name
   * @returns Promise with test result
   */
  async testCustomOpenAIConnection(
    endpoint: string,
    apiKey: string | null,
    model: string
  ): Promise<{ status: string; message: string; http_status?: number }> {
    return testCustomOpenaiConnection({
      endpoint,
      apiKey,
      model,
    });
  }
}

// Export singleton instance
export const configService = new ConfigService();
