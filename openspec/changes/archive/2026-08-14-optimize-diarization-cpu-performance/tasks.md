## 1. Instrumentation and baseline

- [x] 1.1 Add stage timing logs to `run_diarization_blocking` for decode, segmentation, embedding, clustering, and transcript matching
- [x] 1.2 Add peak memory logging using `sysinfo` or `memory-stats` around the diarization run
- [ ] 1.3 Run baseline diarization on a 30-minute stereo test recording and capture wall time, CPU utilization, and peak RSS

## 2. Batch embedding and session pool tuning

- [x] 2.1 Change `ResNet34Adapter::new` construction in `diarization.rs` to accept a configurable `pool_size` instead of hardcoded `1`
- [x] 2.2 Implement `DiarizationConfig` struct (memory mode, max sessions, chunk threshold, chunk overlap) loaded from Tauri settings with sensible defaults
- [x] 2.3 Replace the serial `embed()` loop in `run_polyvoice_diarization` with `embed_batch()` over all segment audio slices
- [x] 2.4 Ensure returned embeddings preserve segment ordering and handle per-segment errors gracefully
- [ ] 2.5 Validate that batch embedding produces bit-identical or equivalent speaker labels compared to the serial path

## 3. Parallel channel processing

- [x] 3.1 Verify `PolyvoiceDiarizer` can be shared across threads (segmenter and embedder pools are `Send + Sync`)
- [x] 3.2 Refactor `run_diarization_blocking` to run microphone and system channel passes concurrently using `rayon::join` or `std::thread::scope`
- [x] 3.3 Ensure cancellation flag is checked safely from both worker threads and the orchestrator
- [ ] 3.4 Validate that parallel channels produce identical speaker labels and timings as sequential runs

## 4. Settings and memory-mode UI

- [x] 4.1 Add `diarization_memory_mode` and `diarization_max_sessions` keys to the Tauri settings store with defaults
- [x] 4.2 Add settings UI controls for memory mode (Auto / Fast / Low memory) and an optional max-session override
- [x] 4.3 Wire settings through to `start_diarization` so the chosen mode affects pool size and chunking
- [x] 4.4 Add backend logic to derive pool size and chunking behavior from mode and system core count

## 5. Chunked processing for long recordings

- [x] 5.1 Implement channel chunking helper that splits a `Vec<f32>` channel into overlapping chunks given duration and overlap parameters
- [x] 5.2 Add `run_chunked_polyvoice_diarization` that iterates chunks, runs segmentation + embedding, and accumulates `(segment, embedding)` pairs
- [x] 5.3 Adjust segment start/end times so they are relative to the full channel after chunking
- [x] 5.4 Run a single global clustering pass over accumulated embeddings after all chunks complete
- [x] 5.5 Gate chunked processing by memory mode and a duration threshold (default 10 minutes in auto mode)
- [ ] 5.6 Validate that a long recording chunked into 10-minute pieces produces speaker labels consistent with a non-chunked run

## 6. Validation and regression testing

- [x] 6.1 Add or update unit tests for chunk splitting and time-offset correction
- [x] 6.2 Add a test that verifies `embed_batch` ordering and error handling
- [ ] 6.3 Run end-to-end diarization on 30-minute, 60-minute, and 2-hour stereo recordings, measuring time and peak memory
- [ ] 6.4 Confirm speaker counts and labels match baseline within acceptable variance (same or better DER on test recordings)
- [ ] 6.5 Verify cancellation works cleanly during batch embedding, parallel channels, and chunked processing

## 7. Documentation and cleanup

- [x] 7.1 Update `AGENTS.md` or relevant docs if diarization behavior/settings changed
- [x] 7.2 Remove any temporary instrumentation or debug logging added during development
- [x] 7.3 Final review of settings defaults and telemetry thresholds
