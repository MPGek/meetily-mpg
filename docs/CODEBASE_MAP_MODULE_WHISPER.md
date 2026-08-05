---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-08-05T14:56:00Z
module: whisper_engine
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Whisper Engine

## Overview

**Purpose**: Local Whisper.cpp transcription via the `whisper-rs` Rust bindings. Manages a global `WhisperEngine` (model catalog discovery, load/unload/delete, streaming download from HuggingFace, adaptive GPU acceleration, and transcription), a Tauri command layer, and a (currently **unwired**) parallel processor for multi-worker chunk transcription.

**Entry point**: `whisper_engine/mod.rs` — module root.

**Sub-packages**: None (single directory).

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mod.rs` | Module root, declares 6 active submodules + re-exports | `crate::whisper_engine::*` | <1k |
| `whisper_engine.rs` | Core engine: model mgmt, download, transcription (`whisper-rs`) | `WhisperEngine`, `ModelInfo`, `ModelStatus` | ~10k |
| `commands.rs` | Tauri commands + global `WHISPER_ENGINE` singleton | `whisper_*` commands, `WHISPER_ENGINE`, `MODELS_DIR` | ~4k |
| `parallel_commands.rs` | Tauri commands for the parallel processor | `ParallelProcessorState`, `*_parallel_processing` | ~2k |
| `parallel_processor.rs` | Multi-worker parallel transcription engine | `ParallelProcessor`, `ParallelConfig`, `ProcessingEvent` | ~4k |
| `acceleration.rs` | GPU backend abstraction | `WhisperCompiledBackend`, `WhisperContextAcceleration` | ~1k |
| `system_monitor.rs` | `sysinfo`-based resource monitoring | `SystemMonitor`, `SystemResources`, `ResourceStatus` | ~2k |
| `_stderr_suppressor.rs` | **Dead file** (entirely commented out, not declared in mod.rs) | — | <1k |

## Public API

### Key Functions (Tauri Commands)

| Function | Signature | Description |
|----------|-----------|-------------|
| `whisper_init` | `() -> Result<(), String>` | Create engine if not present |
| `whisper_get_available_models` | `() -> Result<Vec<ModelInfo>, String>` | Discover models (falls back to `discover_models_standalone` when Parakeet is active) |
| `whisper_load_model` | `(app_handle, model_name) -> Result<(), String>` | Load model; emits `model-loading-started/completed/failed` |
| `whisper_get_current_model` | `() -> Result<Option<String>, String>` | Current model name |
| `whisper_is_model_loaded` | `() -> Result<bool, String>` | Loaded flag |
| `whisper_validate_model_ready` | `() -> Result<String, String>` | Load first available model if none loaded |
| `whisper_transcribe_audio` | `(audio_data: Vec<f32>) -> Result<String, String>` | Transcribe (uses global language preference) |
| `whisper_download_model` | `(app_handle, model_name) -> Result<(), String>` | Stream download; emits `model-download-*` events |
| `whisper_cancel_download` / `whisper_delete_corrupted_model` | `(model_name) -> Result<(), String>` | Cancel / delete |
| `open_models_folder` | `() -> Result<(), String>` | Open models dir in file explorer |

### Key Types

```rust
struct WhisperEngine {
    models_dir: PathBuf,
    current_context: Arc<RwLock<Option<WhisperContext>>>,
    current_model: Arc<RwLock<Option<String>>>,
    available_models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    cancel_download_flag: Arc<RwLock<Option<String>>>,
    active_downloads: Arc<RwLock<HashSet<String>>>,
    // + logging state
}

enum ModelStatus { Available, Missing, Downloading { progress: u8 }, Error(String), Corrupted { .. } }

// Engine methods (all &self, async):
// transcribe_audio(audio: Vec<f32>, language: Option<String>, initial_prompt: Option<String>) -> Result<String>
// transcribe_audio_with_confidence(...) -> Result<(String, f32, bool)>   // (text, avg_confidence, is_partial)
// discover_models() / load_model(name) / unload_model() / download_model(name, cb) / cancel_download(name) / delete_model(name)
```

## Internal Architecture

### Model Lifecycle Flow

1. **Init**: `new_with_models_dir` sets env `GGML_METAL_LOG_LEVEL=1`, `WHISPER_LOG_LEVEL=1`; resolves models dir (dev: `./models`; prod: `data_dir()/Meetily/models`).
2. **Discovery**: `discover_models` scans `WHISPER_MODEL_CATALOG`, validates GGML magic bytes + sizes.
3. **Load**: `load_model` unloads current, calls `HardwareProfile::detect()`, builds `WhisperContextParameters` (`use_gpu`, `gpu_device`, `flash_attn`) via `whisper_context_acceleration_for`.
4. **Transcription**: `transcribe_audio_with_confidence` does beam search, returns `(text, avg_confidence, is_partial)` where `is_partial = duration < adaptive_config.is_partial_threshold_s`.
5. **Download**: streams from HF `ggerganov/whisper.cpp`, progress every ≥1% or ≥2s, honors `cancel_download_flag`, guarded by `active_downloads`.

### Acceleration (`acceleration.rs`)

`WhisperCompiledBackend::current()` priority: `cuda` > `vulkan` > `hipblas` > (macOS `metal`) > `Cpu`. `use_gpu = backend != Cpu`; `flash_attn` only for Metal/Cuda on High/Ultra performance tiers. `gpu_device` always `0`.

### Parallel Processor (`parallel_processor.rs`)

Each worker creates its **own `WhisperEngine` and loads the same model name** (so it's N copies of one model, not multi-model), gated by a tokio `Semaphore` (max 4 workers), 120s per-chunk timeout, retry logic, and resource-monitor auto-pause. **The frontend does NOT call any parallel command** — this path is effectively unused; the live path uses `audio/transcription/worker.rs` (`NUM_WORKERS=1`).

### Concurrency Model

All mutable engine state behind `tokio::sync::RwLock`; transcription serialized on `current_context`. Global `WHISPER_ENGINE` is `std::sync::Mutex<Option<Arc<WhisperEngine>>>`. Note: `transcribe_audio*` takes a **read** lock but calls `create_state()` + `state.full()` — overlapping readers could share one `WhisperContext` (safe today only because the worker serializes access).

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `whisper-rs` | `WhisperContext`, `WhisperContextParameters`, `FullParams`, `SamplingStrategy` | Whisper.cpp bindings |
| `config` | `WHISPER_MODEL_CATALOG` | Model catalog data |
| `audio` | `HardwareProfile`, `GpuType`, `PerformanceTier` | Adaptive GPU config |
| `api::api` | `api_get_transcript_config` | Decide model to load (in `whisper_validate_model_ready_with_config`) |
| `reqwest`, `tokio`, `futures_util` | HTTP download | Model streaming download |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `audio/transcription/engine.rs`, `whisper_provider.rs` | `WHISPER_ENGINE`, `WhisperEngine` | Live transcription |
| `audio/import.rs`, `retranscription.rs`, `common.rs` | `WHISPER_ENGINE` | Import / re-transcribe / unload |
| `audio/recording_commands.rs` | `WHISPER_ENGINE` | Model validation on start |
| `tray.rs`, `lib.rs` | Commands | Registration / tray |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `WHISPER_MODEL_CATALOG` | tiny…large-v3-turbo/large-v3 + q5_1/q5_0 | Available models (config.rs) |
| `DEFAULT_WHISPER_MODEL` | `large-v3` | Config fallback |
| Cargo features | `metal`, `coreml`, `cuda`, `vulkan`, `hipblas`, `openblas`, `openmp` | GPU/BLAS backends (forwarded to whisper-rs) |
| Env | `GGML_METAL_LOG_LEVEL`, `WHISPER_LOG_LEVEL`, `MEMORY_GB` | Logging / memory |
| Adaptive params | from `HardwareProfile::get_whisper_config()` | beam_size, temperature, thresholds, `is_partial_threshold_s`, max_threads |

## Error Handling

- `anyhow::Result` throughout; Tauri commands map to `Result<_, String>`.
- Download paths always clean `active_downloads`; `validate_model_file` checks GGML/GGUF magic.
- Transcription errors if no model loaded. `delete_model` only for `Corrupted`/`Available`.

## Concurrency and Thread Safety

- `tokio::sync::RwLock`-protected engine state; serialized inference on `current_context`.
- Global singletons via `std::sync::Mutex`.
- Parallel workers gated by tokio `Semaphore` (max 4) + 120s timeout + resource auto-pause.

## Gotchas and Tech Debt

- **Race hazard**: read-lock on `current_context` while calling `create_state()` — latent if two transcribe calls overlap.
- **Confidence is fake**: `(length/100.0).min(0.9) + 0.1` — text-length heuristic, not model probability.
- **Parallel path is dead**: fully registered as commands but the frontend never invokes it; `ParallelConfig` memory budget not enforced (Semaphore only limits concurrency).
- **`_stderr_suppressor.rs` is dead** (commented out; not declared). Its former call sites (lines 310, 586) are also commented — if uncommented without re-enabling the module, compilation fails. Env vars now suppress C log spam instead. Recommend deleting.
- **Duplicate logic** between `commands.rs`/`MODELS_DIR` and Parakeet's `commands.rs`; `discover_models_standalone` diverges from `discover_models` (no GGML check, 90%-size rule).
- **`set_no_timestamps(true)` + `set_token_timestamps(true)`** intentionally contradictory (defeats whisper.cpp chunk-skipping heuristics).
- `max_threads` computed but thread-setting code is empty; `whisper_transcribe_audio` has no language/prompt params.
- Cargo feature `openmp` logged but only `metal`/`coreml` enabled by default on macOS.
