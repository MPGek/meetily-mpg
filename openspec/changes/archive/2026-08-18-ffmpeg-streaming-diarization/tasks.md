## 1. Audio metadata probe

- [x] 1.1 Add `probe_audio_metadata(path) -> (sample_rate, channels)` in `decoder.rs` that opens the file with Symphonia and reads track codec params without decoding packets (reuse the probe step at `decoder.rs:487-510`)
- [x] 1.2 Add a unit test that probes a small stereo fixture and a mono fixture and returns the correct channel counts

## 2. ffmpeg streaming decode helper

- [x] 2.1 Add a helper that spawns ffmpeg with `-i <path> -vn -ar 16000 -f f32le pipe:1` (plus `-af "pan=mono|c0=<idx>"` for stereo, `-ac 1` for mono), hides the console window on Windows, and returns the `Child` with piped stdout/stderr
- [x] 2.2 Add a streaming reader that converts raw little-endian f32 stdout bytes into `Vec<f32>` windows of a requested sample count
- [x] 2.3 Drain ffmpeg stderr in a background thread (or `-nostats -loglevel error`) so the pipe cannot deadlock, and surface a useful error when ffmpeg exits non-zero
- [x] 2.4 Add a unit/integration test that decodes a short WAV through the pipe and verifies sample count and approximate content

## 3. Fixed concurrency profile and config simplification

- [x] 3.1 Remove `DiarizationMemoryMode` enum and `DiarizationConfig.memory_mode`, `chunk_threshold_secs` fields from `diarization.rs`
- [x] 3.2 Add a fixed `pool_size()` derivation of `min(8, ceil(0.75 × logical_cpu_count)).max(1)` used by both `segmenter_pool_size()` and `embedder_pool_size()`
- [x] 3.3 Make `should_chunk()`/`chunk_duration_secs()` unconditionally return true / `600.0`
- [x] 3.4 Update `create_polyvoice_diarizer` to use the fixed pool size for both the segmenter and embedder

## 4. Streaming chunked diarization orchestration

- [x] 4.1 Replace the eager `channel_chunks()` with a streaming window reader that reads from the ffmpeg pipe, buffers a 600s window, and carries the 5s overlap forward
- [x] 4.2 Rewrite `run_diarization_blocking` to probe metadata, spawn the per-channel ffmpeg process(es), and drive `run_chunked_polyvoice_diarization` from the stream instead of `decode_audio_file` + `extract_channels`
- [x] 4.3 Remove the Rust `resample()` call from the diarization path (ffmpeg already emits 16 kHz)
- [x] 4.4 Keep global AHC clustering over accumulated embeddings unchanged

## 5. Cancellation of the ffmpeg process

- [x] 5.1 Hold each ffmpeg `Child` handle and call `.kill()` when `DIARIZATION_CANCELLED` is set
- [x] 5.2 Check `DIARIZATION_CANCELLED` between windows and within the read loop so a cancelled job stops promptly

## 6. Symphonia fallback path

- [x] 6.1 When `find_ffmpeg_path()` returns `None`, fall back to the existing `decode_audio_file` + `extract_channels` path with the fixed pool size
- [x] 6.2 Verify the fallback still labels speakers correctly on a mono fixture

## 7. Backend settings cleanup

- [x] 7.1 Remove `memory_mode`/`max_sessions` from `DiarizationSettingsPayload`, `get_diarization_settings`, and `set_diarization_settings`
- [x] 7.2 Remove the `memory_mode`/`max_sessions` params from `start_diarization` and stop reading them from the settings store
- [x] 7.3 Remove the now-unused `load_diarization_config`/`save_diarization_config` store keys (`memory_mode`, `max_sessions`) or reduce the config to the fixed profile only

## 8. Frontend settings cleanup

- [x] 8.1 Remove the memory-mode selector and max-sessions input from `DiarizationSettings.tsx`
- [x] 8.2 Remove `DiarizationMemoryMode` type and the `diarizationMemoryMode`/`diarizationMaxSessions` storage keys from `lib/diarization.ts`
- [x] 8.3 Remove the `memoryMode`/`maxSessions` args from `startDiarization` in `recordingService.ts` and their call sites in `TranscriptContext.tsx` and `useRecordingStop.ts`
- [x] 8.4 Remove the `getDiarizationSettings`/`setDiarizationSettings` memory-mode plumbing that is no longer used

## 9. Tests

- [x] 9.1 Update the chunk-splitting tests in `diarization.rs` to cover the streaming overlap-carry reader
- [x] 9.2 Add a test for the `min(8, ceil(0.75 × cores))` pool-size derivation (floor 1, cap 8)
- [x] 9.3 Verify diarization commands still compile and existing skipped model tests remain valid

## 10. Validation and observability

- [x] 10.1 Run offline diarization on 30-min, 60-min, and 2-hour stereo recordings and confirm peak RSS (logged by `MemorySampler`) drops below ~1.5 GB and no longer scales with duration
- [x] 10.2 Confirm speaker counts and label assignment match the pre-change baseline within acceptable variance
- [x ] 10.3 Confirm cancellation stops the ffmpeg process and leaves partial results in place
- [x] 10.4 Update `AGENTS.md`/docs if diarization settings or memory behavior are described there
