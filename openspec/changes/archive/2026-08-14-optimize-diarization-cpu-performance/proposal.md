## Why

Offline speaker diarization in Meetily is currently CPU-bound on a single core and consumes 3–5 GB of RAM for a 30-minute recording. For two-hour meetings this extrapolates to an unacceptable runtime and memory footprint, causing user-visible delays and potential out-of-memory failures. This change makes diarization scale with available CPU cores and caps memory growth without adding GPU support.

## What Changes

- Switch offline diarization embedding extraction from a serial `embed()` loop to `embed_batch()` with a configurable ONNX session pool size, using all available CPU cores for inference.
- Run microphone and system-channel diarization in parallel on stereo recordings instead of sequentially.
- Expose user-tunable concurrency settings (max embedder/segmenter sessions) and add an automatic "memory-safe mode" that limits in-flight sessions on low-RAM machines.
- Add chunked offline diarization for long recordings: process audio in overlapping temporal windows, accumulate embeddings, and cluster globally so memory stays flat regardless of recording length.
- Add telemetry/logging for diarization stage timing and peak memory so regressions are visible.

## Capabilities

### New Capabilities

_None. All work is an optimization of the existing diarization pipeline._

### Modified Capabilities

- `speaker-diarization`: Add performance and scalability requirements — offline diarization must use multiple CPU cores, must not degrade diarization quality, and must complete without unbounded memory growth for recordings up to two hours.

## Impact

- `frontend/src-tauri/src/audio/diarization.rs` — offline diarization orchestration, batch embedding, channel parallelism, chunking.
- `frontend/src-tauri/src/audio/online_diarization.rs` — shared helper code for speaker matching may be reused or refactored.
- Settings UI and Tauri commands — new concurrency/memory-mode preferences.
- `Cargo.toml` — no new dependencies expected; `rayon` already available.
- Database schema — unchanged.
- User-facing behavior — faster diarization, lower memory, identical speaker labeling quality.
