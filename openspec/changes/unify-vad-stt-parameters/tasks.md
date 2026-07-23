## 1. VadConfig struct

- [x] 1.1 Create `VadConfig` struct in `audio/vad.rs` with all fields (threshold, neg_threshold, min_speech_ms, redemption_ms, pre_pad_ms, post_pad_ms, min_segment_samples, max_segment_samples)
- [x] 1.2 Implement `VadConfig::live()` preset (redemption=200ms, padding=150ms, no max segment)
- [x] 1.3 Implement `VadConfig::batch()` preset (redemption=200ms, padding=150ms, max_segment=25s)
- [x] 1.4 Update `ContinuousVadProcessor::new()` to accept `VadConfig` instead of `redemption_time_ms: u32`
- [x] 1.5 Update `get_speech_chunks()` and `get_speech_chunks_with_progress()` to accept `VadConfig` instead of `redemption_time_ms: u32`

## 2. Segment merger

- [x] 2.1 Implement `merge_segments(segments: &[SpeechSegment], max_gap_ms: u32, max_duration_samples: usize) -> Vec<SpeechSegment>` in `audio/vad.rs`
- [x] 2.2 Add unit test for merger: adjacent segments within gap are merged
- [x] 2.3 Add unit test for merger: distant segments remain separate
- [x] 2.4 Add unit test for merger: merged segment exceeding max duration is split at largest silence gap

## 3. Pipeline live mode

- [x] 3.1 Replace hardcoded `redemption_time` in `AudioPipeline::new()` with `VadConfig::live()`
- [x] 3.2 Remove the `mic_device_kind` and `system_device_kind` parameters from `AudioPipeline::new()` if unused (only needed by the now-removed redemption platform check)

## 4. Retranscription mode

- [x] 4.1 Remove local `VAD_REDEMPTION_TIME_MS` constant from `retranscription.rs`
- [x] 4.2 Replace `get_speech_chunks_with_progress(..., VAD_REDEMPTION_TIME_MS, ...)` with `VadConfig::batch()` parameter
- [x] 4.3 Apply `merge_segments` to VAD output before transcription in `run_retranscription()`

## 5. Import mode

- [x] 5.1 Remove local `VAD_REDEMPTION_TIME_MS` constant from `import.rs`
- [x] 5.2 Replace `get_speech_chunks_with_progress(..., VAD_REDEMPTION_TIME_MS, ...)` with `VadConfig::batch()` parameter
- [x] 5.3 Apply `merge_segments` to VAD output before transcription in import processing

## 6. STT parameters

- [x] 6.1 Move hardcoded `max_len(200)`, `max_initial_ts(1.0)`, `entropy_thold(2.4)`, `is_partial` duration from `whisper_engine.rs` into `AdaptiveWhisperConfig` fields
- [x] 6.2 Use `AdaptiveWhisperConfig` values in `transcribe_audio_with_confidence()` and `transcribe_audio()` instead of inline literals

## 7. Tests

- [x] 7.1 Update `test_vad_400ms_vs_2000ms_segmentation` — the old test compared redemption values directly; rewrite to test merger producing fewer segments than raw VAD
- [x] 7.2 Add test for `VadConfig::live()` preset values
- [x] 7.3 Add test for `VadConfig::batch()` preset values
- [x] 7.4 Verify existing `test_vad_redemption_time_constant` in `retranscription.rs` is updated or removed (it asserted 2000ms, which is no longer the VAD-level constant)
- [x] 7.5 Run full test suite: `cargo test --workspace` — all VAD and retranscription tests must pass
