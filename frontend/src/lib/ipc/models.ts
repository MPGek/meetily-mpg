/**
 * Model management commands and download events: Whisper, Parakeet, built-in
 * summary models, hosted LLM provider model lists, and word-alignment models.
 */
import { invokeTyped, listenTyped, type EventCallback, type UnlistenFn } from './core';

// ---------------------------------------------------------------------------
// Shared args and event payloads
// ---------------------------------------------------------------------------

export interface ModelNameArgs {
  modelName: string;
}

export interface ModelIdArgs {
  modelId: string;
}

/** Rust takes `Vec<f32>`. */
export interface TranscribeAudioArgs {
  audioData: number[];
}

/** `model-download-progress` (Whisper) and `ollama-model-download-progress`. */
export interface ModelDownloadProgressPayload {
  modelName: string;
  progress: number;
}

/** `model-download-complete`, `parakeet-model-download-complete`, `ollama-model-download-complete`. */
export interface ModelDownloadCompletePayload {
  modelName: string;
}

/** `model-download-error`, `parakeet-model-download-error`, `ollama-model-download-error`. */
export interface ModelDownloadErrorPayload {
  modelName: string;
  error: string;
}

// ---------------------------------------------------------------------------
// Whisper / Parakeet model types
// ---------------------------------------------------------------------------

export type ModelAccuracy = 'High' | 'Good' | 'Decent';

/** Every speed value in the Rust `WHISPER_MODEL_CATALOG` is one of these. */
export type ProcessingSpeed = 'Slow' | 'Medium' | 'Fast' | 'Very Fast';

/**
 * Externally tagged Rust enum, shared by the Whisper and Parakeet engines
 * (identical definitions in whisper_engine.rs and parakeet_engine.rs).
 *
 * Known drift: Rust serializes `Downloading { progress }` as
 * `{ Downloading: { progress: n } }`, but the model managers build and read
 * `{ Downloading: n }` locally from download events. The frontend shape is kept
 * here so this change stays type-only; reconciling it is a follow-up.
 */
export type ModelStatus =
  | 'Available'
  | 'Missing'
  | { Downloading: number }
  | { Error: string }
  | { Corrupted: { file_size: number; expected_min_size: number } };

/** Whisper `ModelInfo` (whisper_engine.rs). */
export interface ModelInfo {
  name: string;
  path: string;
  size_mb: number;
  accuracy: ModelAccuracy;
  speed: ProcessingSpeed;
  status: ModelStatus;
  description: string;
}

export type QuantizationType = 'FP32' | 'Int8';

/** Parakeet `ModelInfo` (parakeet_engine.rs). */
export interface ParakeetModelInfo {
  name: string;
  path: string;
  size_mb: number;
  /** Free text, e.g. "Ultra Fast (v3)". */
  speed: string;
  status: ModelStatus;
  description: string;
  quantization: QuantizationType;
  accuracy?: ModelAccuracy; // not sent by Rust
}

/**
 * `parakeet-model-download-progress`. The byte/MB/speed fields are absent on
 * the `cancelled` event emitted by `parakeet_cancel_download`.
 */
export interface ParakeetModelDownloadProgressPayload {
  modelName: string;
  progress: number;
  downloaded_bytes?: number;
  total_bytes?: number;
  downloaded_mb?: number;
  total_mb?: number;
  speed_mbps?: number;
  status: 'downloading' | 'completed' | 'cancelled';
}

// ---------------------------------------------------------------------------
// Whisper
// ---------------------------------------------------------------------------

export async function whisperInit(): Promise<void> {
  return invokeTyped<void>('whisper_init');
}

export async function whisperLoadModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('whisper_load_model', args);
}

export async function whisperGetAvailableModels(): Promise<ModelInfo[]> {
  return invokeTyped<ModelInfo[]>('whisper_get_available_models');
}

export async function whisperGetCurrentModel(): Promise<string | null> {
  return invokeTyped<string | null>('whisper_get_current_model');
}

export async function whisperGetModelsDirectory(): Promise<string> {
  return invokeTyped<string>('whisper_get_models_directory');
}

export async function whisperHasAvailableModels(): Promise<boolean> {
  return invokeTyped<boolean>('whisper_has_available_models');
}

export async function whisperIsModelLoaded(): Promise<boolean> {
  return invokeTyped<boolean>('whisper_is_model_loaded');
}

export async function whisperValidateModelReady(): Promise<string> {
  return invokeTyped<string>('whisper_validate_model_ready');
}

export async function whisperDownloadModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('whisper_download_model', args);
}

export async function whisperCancelDownload(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('whisper_cancel_download', args);
}

export async function whisperDeleteCorruptedModel(args: ModelNameArgs): Promise<string> {
  return invokeTyped<string>('whisper_delete_corrupted_model', args);
}

export async function whisperTranscribeAudio(args: TranscribeAudioArgs): Promise<string> {
  return invokeTyped<string>('whisper_transcribe_audio', args);
}

export async function openModelsFolder(): Promise<void> {
  return invokeTyped<void>('open_models_folder');
}

export function listenModelDownloadProgress(
  handler: EventCallback<ModelDownloadProgressPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadProgressPayload>('model-download-progress', handler);
}

export function listenModelDownloadComplete(
  handler: EventCallback<ModelDownloadCompletePayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadCompletePayload>('model-download-complete', handler);
}

export function listenModelDownloadError(
  handler: EventCallback<ModelDownloadErrorPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadErrorPayload>('model-download-error', handler);
}

// ---------------------------------------------------------------------------
// Parakeet
// ---------------------------------------------------------------------------

export async function parakeetInit(): Promise<void> {
  return invokeTyped<void>('parakeet_init');
}

export async function parakeetGetAvailableModels(): Promise<ParakeetModelInfo[]> {
  return invokeTyped<ParakeetModelInfo[]>('parakeet_get_available_models');
}

export async function parakeetGetCurrentModel(): Promise<string | null> {
  return invokeTyped<string | null>('parakeet_get_current_model');
}

export async function parakeetGetModelsDirectory(): Promise<string> {
  return invokeTyped<string>('parakeet_get_models_directory');
}

export async function parakeetHasAvailableModels(): Promise<boolean> {
  return invokeTyped<boolean>('parakeet_has_available_models');
}

export async function parakeetIsModelLoaded(): Promise<boolean> {
  return invokeTyped<boolean>('parakeet_is_model_loaded');
}

export async function parakeetValidateModelReady(): Promise<string> {
  return invokeTyped<string>('parakeet_validate_model_ready');
}

export async function parakeetLoadModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('parakeet_load_model', args);
}

export async function parakeetDownloadModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('parakeet_download_model', args);
}

export async function parakeetRetryDownload(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('parakeet_retry_download', args);
}

export async function parakeetCancelDownload(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('parakeet_cancel_download', args);
}

export async function parakeetDeleteCorruptedModel(args: ModelNameArgs): Promise<string> {
  return invokeTyped<string>('parakeet_delete_corrupted_model', args);
}

export async function parakeetTranscribeAudio(args: TranscribeAudioArgs): Promise<string> {
  return invokeTyped<string>('parakeet_transcribe_audio', args);
}

export async function openParakeetModelsFolder(): Promise<void> {
  return invokeTyped<void>('open_parakeet_models_folder');
}

export function listenParakeetModelDownloadProgress(
  handler: EventCallback<ParakeetModelDownloadProgressPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ParakeetModelDownloadProgressPayload>(
    'parakeet-model-download-progress',
    handler,
  );
}

export function listenParakeetModelDownloadComplete(
  handler: EventCallback<ModelDownloadCompletePayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadCompletePayload>('parakeet-model-download-complete', handler);
}

export function listenParakeetModelDownloadError(
  handler: EventCallback<ModelDownloadErrorPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadErrorPayload>('parakeet-model-download-error', handler);
}

// ---------------------------------------------------------------------------
// Built-in AI (summary models)
// ---------------------------------------------------------------------------

/**
 * Internally tagged (`type`, snake_case) Rust enum. The Rust `Error(String)`
 * variant cannot be serialized under internal tagging, so it never arrives;
 * `Error` is kept optional for existing readers.
 */
export type BuiltInModelStatus =
  | { type: 'not_downloaded' }
  | { type: 'downloading'; progress: number }
  | { type: 'available' }
  | { type: 'corrupted'; file_size: number; expected_min_size: number }
  | { type: 'error'; Error?: string }; // not sent by Rust

/** `ModelInfo` in summary/summary_engine/model_manager.rs. */
export interface BuiltInModelInfo {
  name: string;
  display_name: string;
  status: BuiltInModelStatus;
  path: string;
  size_mb: number;
  context_size: number;
  description: string;
  gguf_file: string;
}

/**
 * `builtin-ai-download-progress`. Carries progress (`downloading`), completion
 * (`completed`), failure (`error` + `error` message) and cancellation
 * (`cancelled`, without the MB/speed fields).
 */
export interface BuiltInDownloadProgressPayload {
  model: string;
  progress: number;
  downloaded_mb?: number;
  total_mb?: number;
  speed_mbps?: number;
  status: 'downloading' | 'completed' | 'error' | 'cancelled';
  error?: string;
}

export interface BuiltinAiIsModelReadyArgs {
  modelName: string;
  refresh?: boolean | null;
}

export async function builtinAiListModels(): Promise<BuiltInModelInfo[]> {
  return invokeTyped<BuiltInModelInfo[]>('builtin_ai_list_models');
}

export async function builtinAiDownloadModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('builtin_ai_download_model', args);
}

export async function builtinAiCancelDownload(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('builtin_ai_cancel_download', args);
}

export async function builtinAiDeleteModel(args: ModelNameArgs): Promise<void> {
  return invokeTyped<void>('builtin_ai_delete_model', args);
}

export async function builtinAiGetModelInfo(args: ModelNameArgs): Promise<BuiltInModelInfo | null> {
  return invokeTyped<BuiltInModelInfo | null>('builtin_ai_get_model_info', args);
}

export async function builtinAiIsModelReady(args: BuiltinAiIsModelReadyArgs): Promise<boolean> {
  return invokeTyped<boolean>('builtin_ai_is_model_ready', args);
}

export async function builtinAiGetAvailableSummaryModel(): Promise<string | null> {
  return invokeTyped<string | null>('builtin_ai_get_available_summary_model');
}

export async function builtinAiGetRecommendedModel(): Promise<string> {
  return invokeTyped<string>('builtin_ai_get_recommended_model');
}

/**
 * No Rust command named `builtin_ai_get_models_directory` is registered, so
 * this always rejects, as the direct invoke did before. Kept for parity.
 */
export async function builtinAiGetModelsDirectory(): Promise<string> {
  return invokeTyped<string>('builtin_ai_get_models_directory');
}

export function listenBuiltinAiDownloadProgress(
  handler: EventCallback<BuiltInDownloadProgressPayload>,
): Promise<UnlistenFn> {
  return listenTyped<BuiltInDownloadProgressPayload>('builtin-ai-download-progress', handler);
}

// ---------------------------------------------------------------------------
// Hosted providers (Ollama, Anthropic, Groq, OpenAI, OpenRouter)
// ---------------------------------------------------------------------------

export interface OllamaModel {
  name: string;
  id: string;
  size: string;
  modified: string;
}

export interface AnthropicModel {
  id: string;
  display_name: string | null;
}

export interface GroqModel {
  id: string;
  owned_by: string | null;
}

export interface OpenAIModel {
  id: string;
}

export interface OpenRouterModel {
  id: string;
  name: string;
  context_length: number | null;
  prompt_price: string | null;
  completion_price: string | null;
}

export interface GetOllamaModelsArgs {
  endpoint?: string | null;
}

export interface PullOllamaModelArgs {
  modelName: string;
  endpoint?: string | null;
}

export interface ProviderApiKeyArgs {
  apiKey?: string | null;
}

export async function getOllamaModels(args: GetOllamaModelsArgs): Promise<OllamaModel[]> {
  return invokeTyped<OllamaModel[]>('get_ollama_models', args);
}

/** Resolves when the pull finishes; progress arrives via `ollama-model-download-*`. */
export async function pullOllamaModel(args: PullOllamaModelArgs): Promise<void> {
  return invokeTyped<void>('pull_ollama_model', args);
}

export async function getAnthropicModels(args: ProviderApiKeyArgs): Promise<AnthropicModel[]> {
  return invokeTyped<AnthropicModel[]>('get_anthropic_models', args);
}

export async function getGroqModels(args: ProviderApiKeyArgs): Promise<GroqModel[]> {
  return invokeTyped<GroqModel[]>('get_groq_models', args);
}

export async function getOpenaiModels(args: ProviderApiKeyArgs): Promise<OpenAIModel[]> {
  return invokeTyped<OpenAIModel[]>('get_openai_models', args);
}

export async function getOpenrouterModels(): Promise<OpenRouterModel[]> {
  return invokeTyped<OpenRouterModel[]>('get_openrouter_models');
}

export function listenOllamaModelDownloadProgress(
  handler: EventCallback<ModelDownloadProgressPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadProgressPayload>('ollama-model-download-progress', handler);
}

export function listenOllamaModelDownloadComplete(
  handler: EventCallback<ModelDownloadCompletePayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadCompletePayload>('ollama-model-download-complete', handler);
}

export function listenOllamaModelDownloadError(
  handler: EventCallback<ModelDownloadErrorPayload>,
): Promise<UnlistenFn> {
  return listenTyped<ModelDownloadErrorPayload>('ollama-model-download-error', handler);
}

// ---------------------------------------------------------------------------
// Word-alignment models
// ---------------------------------------------------------------------------

/** Adjacently tagged (`state` + `detail`) on the Rust side. */
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

export interface AlignmentDownloadCompletedPayload {
  modelId: string;
}

export interface AlignmentDownloadFailedPayload {
  modelId: string;
  error: string;
}

export async function listAlignmentModels(): Promise<AlignmentModelInfo[]> {
  return invokeTyped<AlignmentModelInfo[]>('list_alignment_models');
}

export async function checkAlignmentModels(): Promise<Record<string, AlignmentModelStatus>> {
  return invokeTyped<Record<string, AlignmentModelStatus>>('check_alignment_models');
}

/** Resolves when the download completes; progress arrives via `alignment-model-download-*`. */
export async function downloadAlignmentModel(args: ModelIdArgs): Promise<void> {
  return invokeTyped<void>('download_alignment_model', args);
}

export async function cancelAlignmentDownload(args: ModelIdArgs): Promise<void> {
  return invokeTyped<void>('cancel_alignment_download', args);
}

export async function deleteAlignmentModel(args: ModelIdArgs): Promise<void> {
  return invokeTyped<void>('delete_alignment_model', args);
}

export function listenAlignmentModelDownloadProgress(
  handler: EventCallback<AlignmentDownloadProgressPayload>,
): Promise<UnlistenFn> {
  return listenTyped<AlignmentDownloadProgressPayload>(
    'alignment-model-download-progress',
    handler,
  );
}

export function listenAlignmentModelDownloadCompleted(
  handler: EventCallback<AlignmentDownloadCompletedPayload>,
): Promise<UnlistenFn> {
  return listenTyped<AlignmentDownloadCompletedPayload>(
    'alignment-model-download-completed',
    handler,
  );
}

export function listenAlignmentModelDownloadFailed(
  handler: EventCallback<AlignmentDownloadFailedPayload>,
): Promise<UnlistenFn> {
  return listenTyped<AlignmentDownloadFailedPayload>('alignment-model-download-failed', handler);
}
