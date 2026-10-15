## Why

Offline speaker diarization decodes the entire recording into RAM up front and then makes multiple full-size copies (de-interleave, eager chunk materialization, resample input clone), so a 50-minute meeting peaks at 5–7 GB. The existing memory-mode setting (`auto`/`fast`/`low_memory`) does not fix this because chunking runs *after* full decode, and the toggle itself is confusing — it asks users to choose a memory/performance tradeoff that should be an internal decision.

## What Changes

- **Stream audio decode through ffmpeg stdout** instead of decoding the whole file into a `Vec<f32>` first. ffmpeg performs decode → 16 kHz resample → stereo channel split in one native pass, and Rust consumes the pipe in bounded in-memory chunks, so peak memory becomes flat regardless of recording length.
- **Remove the `memory_mode` setting** (`auto`/`fast`/`low_memory`) and the `max_sessions` override from the backend config, the settings store, and the settings UI. Diarization uses a single fixed behavior with no user-facing memory/performance knob.
- **Fix the ONNX session pool size** to `min(8, ceil(0.75 × CPU core count))` for both the segmenter and embedder pools — the "fast enough, memory-safe" balance formerly approximated by the modes.
- **Keep chunked diarization with in-memory audio chunks** (no temp audio files): process each channel in overlapping chunks, accumulating only embeddings and segment metadata, then run one global clustering pass.
- **Remove the eager copy chain** that inflates peak memory: drop the decoded interleaved buffer after channel extraction, stream chunks lazily rather than materializing them all up front, and avoid cloning the input inside the resampler.
- **BREAKING**: The `diarizationMemoryMode` and `diarizationMaxSessions` settings keys are removed. Existing stored values are ignored; no migration is needed.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `speaker-diarization`: Removes the "Configurable concurrency and memory mode" requirement (auto/fast/low-memory modes and the max-session override), replaces it with a fixed single-mode behavior, and changes the decode path so audio is streamed via ffmpeg rather than fully decoded into memory. The "long recordings" and "chunked diarization" requirements are reworded so the chunk threshold is fixed rather than user-configurable.

## Impact

- **Backend (Rust)**: `frontend/src-tauri/src/audio/diarization.rs` (remove `DiarizationMemoryMode`, `DiarizationConfig.memory_mode`, mode-derived chunk/pool logic, and settings commands; add fixed pool-size derivation and ffmpeg-pipe streaming), `frontend/src-tauri/src/audio/decoder.rs` and `audio_processing.rs` (streaming decode path, resample without input clone), `frontend/src-tauri/src/audio/ffmpeg.rs` (pipe-spawning helper).
- **Frontend (TS/React)**: `lib/diarization.ts`, `components/DiarizationSettings.tsx`, `services/recordingService.ts`, `contexts/TranscriptContext.tsx`, `hooks/useRecordingStop.ts` (remove memory-mode and max-session controls and plumbing).
- **Dependencies**: `ffmpeg_sidecar`/bundled ffmpeg already present; no new dependencies.
- **No database schema changes.**
