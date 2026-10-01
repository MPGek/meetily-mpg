/**
 * Settings IPC: summary/transcript model configuration, custom OpenAI
 * endpoint, notifications, language, word alignment, folders, console.
 */
import { emitTyped, invokeTyped, listenTyped, type EventCallback, type UnlistenFn } from './core';

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

export interface ModelConfig {
  /** Rust sends a plain `String`; narrowed to the providers the UI knows. */
  provider: 'ollama' | 'groq' | 'claude' | 'openrouter' | 'openai' | 'builtin-ai' | 'custom-openai';
  model: string;
  whisperModel: string;
  /**
   * @deprecated Use providerApiKeys from ConfigContext instead.
   * This field may contain stale data when provider changes without saving.
   */
  apiKey?: string | null;
  ollamaEndpoint?: string | null;
  // Custom OpenAI fields, filled in by the frontend from CustomOpenAIConfig
  customOpenAIEndpoint?: string | null; // not sent by Rust
  customOpenAIModel?: string | null; // not sent by Rust
  customOpenAIApiKey?: string | null; // not sent by Rust
  maxTokens?: number | null; // not sent by Rust
  temperature?: number | null; // not sent by Rust
  topP?: number | null; // not sent by Rust
}

export interface TranscriptModelProps {
  /** Rust sends a plain `String`; narrowed to the providers the UI knows. */
  provider: 'localWhisper' | 'parakeet' | 'deepgram' | 'elevenLabs' | 'groq' | 'openai';
  model: string;
  apiKey?: string | null;
}

export interface CustomOpenAIConfig {
  endpoint: string;
  apiKey: string | null;
  model: string;
  maxTokens: number | null;
  temperature: number | null;
  topP: number | null;
}

/** `{ status, message }` JSON returned by the save commands. */
export interface ConfigSaveResult {
  status: string;
  message: string;
}

/** Only the success shape: every failure rejects with the error string. */
export interface CustomOpenAIConnectionTestResult {
  status: string;
  message: string;
  http_status: number;
}

export interface NotificationSettings {
  recording_notifications: boolean;
  time_based_reminders: boolean;
  meeting_reminders: boolean;
  respect_do_not_disturb: boolean;
  notification_sound: boolean;
  system_permission_granted: boolean;
  consent_given: boolean;
  manual_dnd_mode: boolean;
  notification_preferences: {
    show_recording_started: boolean;
    show_recording_stopped: boolean;
    show_recording_paused: boolean;
    show_recording_resumed: boolean;
    show_transcription_complete: boolean;
    show_meeting_reminders: boolean;
    show_system_errors: boolean;
    meeting_reminder_minutes: number[];
  };
}

// ---------------------------------------------------------------------------
// Summary model configuration
// ---------------------------------------------------------------------------

export interface SaveModelConfigArgs {
  provider: string;
  model: string;
  whisperModel: string;
  apiKey?: string | null;
  ollamaEndpoint?: string | null;
}

/** Resolves `null` when the settings table has no model config yet. */
export async function getModelConfig(): Promise<ModelConfig | null> {
  return invokeTyped<ModelConfig | null>('api_get_model_config');
}

export async function saveModelConfig(args: SaveModelConfigArgs): Promise<ConfigSaveResult> {
  return invokeTyped<ConfigSaveResult>('api_save_model_config', args);
}

export interface ProviderArgs {
  provider: string;
}

/** Resolves `''` when no key is stored for the provider. */
export async function getApiKey(args: ProviderArgs): Promise<string> {
  return invokeTyped<string>('api_get_api_key', args);
}

/**
 * @remarks Not registered: `api_get_auto_generate_setting` is commented out of
 * `generate_handler!` in lib.rs and has no Rust fn, so this always rejects.
 */
export async function getAutoGenerateSetting(): Promise<boolean> {
  return invokeTyped<boolean>('api_get_auto_generate_setting');
}

// ---------------------------------------------------------------------------
// Custom OpenAI-compatible endpoint
// ---------------------------------------------------------------------------

export interface SaveCustomOpenAIConfigArgs {
  endpoint: string;
  apiKey?: string | null;
  model: string;
  maxTokens?: number | null;
  temperature?: number | null;
  topP?: number | null;
}

export interface TestCustomOpenAIConnectionArgs {
  endpoint: string;
  apiKey?: string | null;
  model: string;
}

export async function getCustomOpenaiConfig(): Promise<CustomOpenAIConfig | null> {
  return invokeTyped<CustomOpenAIConfig | null>('api_get_custom_openai_config');
}

export async function saveCustomOpenaiConfig(
  args: SaveCustomOpenAIConfigArgs,
): Promise<ConfigSaveResult> {
  return invokeTyped<ConfigSaveResult>('api_save_custom_openai_config', args);
}

export async function testCustomOpenaiConnection(
  args: TestCustomOpenAIConnectionArgs,
): Promise<CustomOpenAIConnectionTestResult> {
  return invokeTyped<CustomOpenAIConnectionTestResult>('api_test_custom_openai_connection', args);
}

// ---------------------------------------------------------------------------
// Transcript model configuration
// ---------------------------------------------------------------------------

export interface SaveTranscriptConfigArgs {
  provider: string;
  model: string;
  apiKey?: string | null;
}

/** Rust returns a parakeet default instead of `null` when nothing is saved. */
export async function getTranscriptConfig(): Promise<TranscriptModelProps | null> {
  return invokeTyped<TranscriptModelProps | null>('api_get_transcript_config');
}

export async function saveTranscriptConfig(args: SaveTranscriptConfigArgs): Promise<ConfigSaveResult> {
  return invokeTyped<ConfigSaveResult>('api_save_transcript_config', args);
}

/** Resolves `''` when no key is stored for the provider. */
export async function getTranscriptApiKey(args: ProviderArgs): Promise<string> {
  return invokeTyped<string>('api_get_transcript_api_key', args);
}

// ---------------------------------------------------------------------------
// Language, notifications, word alignment
// ---------------------------------------------------------------------------

export interface SetLanguagePreferenceArgs {
  language: string;
}

/** Registered from a non-pub fn in lib.rs; updates the in-memory live-recording language. */
export async function setLanguagePreference(args: SetLanguagePreferenceArgs): Promise<void> {
  return invokeTyped<void>('set_language_preference', args);
}

export async function getNotificationSettings(): Promise<NotificationSettings> {
  return invokeTyped<NotificationSettings>('get_notification_settings');
}

export interface SetNotificationSettingsArgs {
  settings: NotificationSettings;
}

export async function setNotificationSettings(args: SetNotificationSettingsArgs): Promise<void> {
  return invokeTyped<void>('set_notification_settings', args);
}

export interface SetWordAlignmentSettingsArgs {
  enabled: boolean;
  modelId?: string | null;
}

export async function setWordAlignmentSettings(args: SetWordAlignmentSettingsArgs): Promise<void> {
  return invokeTyped<void>('set_word_alignment_settings', args);
}

// ---------------------------------------------------------------------------
// Folders, system settings, external links
// ---------------------------------------------------------------------------

export async function getDatabaseDirectory(): Promise<string> {
  return invokeTyped<string>('get_database_directory');
}

export async function openDatabaseFolder(): Promise<void> {
  return invokeTyped<void>('open_database_folder');
}

export async function openRecordingsFolder(): Promise<void> {
  return invokeTyped<void>('open_recordings_folder');
}

export interface OpenSystemSettingsArgs {
  preferencePane?: string;
}

/**
 * Registered on macOS only (`#[cfg(target_os = "macos")]`), so it rejects
 * elsewhere. Rust requires `preferencePane`; the onboarding call sites omit
 * it, so those calls reject (and are caught) as today.
 */
export async function openSystemSettings(args?: OpenSystemSettingsArgs): Promise<void> {
  return invokeTyped<void>('open_system_settings', args);
}

export interface OpenExternalUrlArgs {
  url: string;
}

export async function openExternalUrl(args: OpenExternalUrlArgs): Promise<void> {
  return invokeTyped<void>('open_external_url', args);
}

// ---------------------------------------------------------------------------
// Console window (Windows; resolves a status message)
// ---------------------------------------------------------------------------

export async function toggleConsole(): Promise<string> {
  return invokeTyped<string>('toggle_console');
}

export async function showConsole(): Promise<string> {
  return invokeTyped<string>('show_console');
}

export async function hideConsole(): Promise<string> {
  return invokeTyped<string>('hide_console');
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/** Emitted by the frontend itself (`emit('model-config-updated', config)`), not by Rust. */
export function listenModelConfigUpdated(handler: EventCallback<ModelConfig>): Promise<UnlistenFn> {
  return listenTyped<ModelConfig>('model-config-updated', handler);
}

/** `model-config-updated` is emitted by the frontend itself, never by Rust. */
export function emitModelConfigUpdated(payload: ModelConfig): Promise<void> {
  return emitTyped<ModelConfig>('model-config-updated', payload);
}
