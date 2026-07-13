---
parent: CODEBASE_MAP_MODULES.md
last_mapped: 2026-07-13T14:31:00Z
module: parakeet_engine
---

> Part of [Module Guide](CODEBASE_MAP_MODULES.md) | [Codebase Map](CODEBASE_MAP.md)

# Module: Parakeet Engine

## Overview

**Purpose**: The Parakeet engine module provides real-time speech-to-text transcription using the Parakeet ONNX model. Unlike Whisper.cpp which uses GGUF format, Parakeet uses ONNX runtime for inference. It's designed for lower-latency transcription with smaller model footprint, suitable for real-time streaming scenarios.

**Entry point**: `parakeet_engine/mod.rs` — module root
**Sub-packages**: None (single directory)

## File Reference

| File | Purpose | Key Exports | Tokens |
|------|---------|-------------|--------|
| `mod.rs` | Module root, re-exports all sub-modules | parakeet types, commands | ~1k |
| `parakeet_engine.rs` | Core ONNX inference wrapper | ParakeetEngine struct, load/inference | ~8k |
| `commands.rs` | Tauri command handlers | parakeet_init, transcribe, model management | ~5k |
| `model.rs` | Model metadata and versioning | ModelInfo, version checking | ~3k |

## Public API

### Key Functions (Tauri Commands)

| Function | Signature | Description |
|----------|-----------|-------------|
| `parakeet_init` | `(app) -> Result<(), String>` | Initialize Parakeet engine and load model |
| `parakeet_unload_model` | `() -> Result<(), String>` | Unload current Parakeet model from memory |
| `parakeet_is_model_loaded` | `() -> bool` | Check if model is currently loaded |
| `parakeet_transcribe_audio` | `(audio_path) -> Result<TranscriptionResult, String>` | Transcribe audio using Parakeet ONNX model |
| `parakeet_get_available_models` | `() -> Vec<ModelInfo>` | List available Parakeet models |
| `parakeet_download_model` | `(model_name) -> Result<(), String>` | Download Parakeet model from HuggingFace |

### Key Types

```rust
struct ParakeetEngine {
    session: ort::Session,
    model_path: PathBuf,
}

struct ModelInfo {
    name: String,
    version: String,
    path: PathBuf,
    size_bytes: u64,
}
```

## Internal Architecture

### ONNX Inference Flow

1. **Model Loading**: ONNX session created from model file via `ort::Session::builder().commit_from_read()`
2. **Preprocessing**: Audio samples converted to ONNX-compatible tensor format (float32)
3. **Inference**: ONNX runtime executes model on CPU (no GPU acceleration currently)
4. **Post-processing**: Raw output decoded to text with timestamp alignment

### Concurrency Model

- Model loading in tokio spawn block
- Inference runs synchronously within async context
- No parallel processing (single-threaded inference per chunk)

## Dependencies (imports FROM)

| Module/Package | What is imported | Why |
|---------------|-----------------|-----|
| `ort` | `Session`, `Tensor` | ONNX Runtime for model inference |
| `tokio` | `spawn` | Async model loading |

## Dependents (imported BY)

| Consumer Module | What it uses | Context |
|----------------|-------------|---------|
| `audio/transcription/` | Transcribe audio chunks | Real-time transcription during recording |
| `lib.rs` (main) | All Tauri commands | Entry point for frontend control |

## Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `model_name` | parakeet-v1.0 | Default Parakeet model version |
| `sample_rate` | 16000 Hz | Expected input sample rate |

## Error Handling

- **Model not found**: Returns error with path to models directory
- **Invalid ONNX file**: Deleted and re-downloaded via cleanup command
- **Inference failure**: Returns error with model-specific details

## Gotchas and Tech Debt

- **CPU-only**: No GPU acceleration support yet (unlike Whisper)
- **Model size**: Smaller than Whisper but potentially less accurate for complex audio
- **Language support**: May have limited language coverage compared to Whisper