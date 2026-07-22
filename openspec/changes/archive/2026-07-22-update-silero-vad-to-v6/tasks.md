## 1. Extract v6 ONNX Model

- [x] 1.1 Run `uv run --with silero-vad==6.2 --python 3.12 python -c "..."` to extract the v6 ONNX model from the silero-vad Python package
- [x] 1.2 Verify model loads with `ort` by running a basic inference test (feed zeros, confirm output is a probability ~0.0-0.1)
- [x] 1.3 Compare model metadata (opset, input/output shapes) against the existing v4 model using `onnx` library

## 2. Create build.rs Model Embedding

- [x] 2.1 Create `frontend/src-tauri/build.rs` that downloads/extracts the v6 ONNX model at build time
- [x] 2.2 Embed model bytes using `include_bytes!` so it's compiled into the binary
- [x] 2.3 Add fallback mechanism: if model extraction fails, download from GitHub releases
- [x] 2.4 Document `uv` prerequisite in the project README (build.rs prints instructions if uv is missing)

## 3. Implement VadSessionV6 (Replace silero-rs)

- [x] 3.1 Create `VadSessionV6` struct in `audio/vad.rs` with fields: `session: ort::Session`, `state: ndarray::Array3<f32>` [2,1,128], `context: VecDeque<f32>`, `sample_rate: usize`
- [x] 3.2 Implement `VadSessionV6::new(config)` that loads the embedded ONNX model and initializes zero state
- [x] 3.3 Implement `forward()` — assemble 576-sample input (512 + 64 context), run ort inference with `input`, `state`, `sr` tensors, extract output probability and updated state
- [x] 3.4 Implement context management — after each `forward()`, save last 64 samples of the input chunk for next call; on first call use zero context
- [x] 3.5 Implement `reset()` — zero the state tensor and clear the context buffer
- [x] 3.6 Ensure `ort` session uses `GraphOptimizationLevel::Level3` and appropriate thread count (match existing `silero_rs` settings)

## 4. Update ContinuousVadProcessor

- [x] 4.1 Change `VAD_SAMPLE_RATE` constant from 16000 (no change — v6 also uses 16kHz)
- [x] 4.2 Change `vad_chunk_size` from 480 (30ms) to 512 (32ms) to match v6 fixed window
- [x] 4.3 Replace `VadSession` (from silero_rs) with `VadSessionV6` in the `ContinuousVadProcessor` struct
- [x] 4.4 Update `process_chunk()` — no longer call `session.process()`, instead call new `forward()` and extract transition events from probability stream
- [x] 4.5 Rebuild the transition logic: v6 returns raw probabilities per chunk (not transitions), so reimplement `SpeechStart`/`SpeechEnd` detection using the threshold windowing approach (positive_speech_threshold 0.50, negative_speech_threshold 0.35)

## 5. Remove silero-rs Dependency

- [x] 5.1 Remove `silero_rs = { git = ... }` from `Cargo.toml`
- [x] 5.2 Remove all `use silero_rs::*` imports from `audio/vad.rs`
- [ ] 5.3 Remove unused silero-rs git checkout from `.cargo/git/checkouts/` (optional cleanup)
- [x] 5.4 Verify `ort` is listed as a direct dependency in `Cargo.toml` (it may already be transitive)
- [x] 5.5 Run `cargo check` to confirm compilation succeeds

## 6. Update VAD Unit Tests

- [ ] 6.1 Update `test_vad_chunked_vs_single_processing` — verify v6 chunked vs single processing still produces same segment count
- [ ] 6.2 Update `test_vad_400ms_vs_2000ms_segmentation` — verify redemption time behavior with v6 model
- [ ] 6.3 Update `test_vad_continuous_processor_state_across_chunks` — verify state continuity across chunk boundaries
- [x] 6.4 Add test: verify context prepending by processing two sequential chunks and checking that the second chunk's probability differs from processing alone
- [x] 6.5 Add test: verify model loaded correctly by checking output probability range (0.0-1.0) for known inputs (silence → ~0.0, speech-like noise → ~0.5+)

## 7. Validate Against Python Reference

- [ ] 7.1 Generate a test WAV file with varied audio (silence, speech, music, noise) using Python
- [ ] 7.2 Run the same WAV through Python silero-vad v6 `get_speech_timestamps()` and capture per-chunk probabilities
- [ ] 7.3 Run the same WAV through the Rust v6 implementation and capture per-chunk probabilities
- [ ] 7.4 Compare probability streams — they should match within ~1e-3 tolerance (small differences expected from platform-specific ONNX runtime optimizations)
- [ ] 7.5 Fix any significant discrepancies found

## 8. Integration Test

- [ ] 8.1 Run the full recording → VAD → transcription pipeline with synthetic audio
- [ ] 8.2 Verify that `SpeechSegment` timestamps and sample arrays are reasonable
- [ ] 8.3 Verify that the `extract_speech_16k()` convenience function still works (it uses `ContinuousVadProcessor` internally)
- [ ] 8.4 Verify no performance regression on large files (120s+ audio) — v6 should be comparable or faster
- [ ] 8.5 Run `cargo test` to confirm all tests pass
