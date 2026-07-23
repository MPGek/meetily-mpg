## 1. Pipeline segment accumulation

- [x] 1.1 Add segment accumulation buffer (`Vec<SpeechSegment>`) to `AudioPipeline` struct
- [x] 1.2 In `AudioPipeline::run()`, collect VAD segments into the buffer instead of dispatching individually
- [x] 1.3 When a segment arrives with gap ≥ 500ms from the last accumulated segment, call `merge_segments(&buffer, 500.0, 25*16000)` and dispatch all merged chunks
- [x] 1.4 On recording flush signal, merge+dispatch any remaining accumulated segments
- [x] 1.5 Remove the per-segment `samples.len() >= 800` filter; use `min_segment_samples = 1600` from `VadConfig::live()`

## 2. Transcription worker context forwarding

- [x] 2.1 Add `last_mic_text: String` and `last_sys_text: String` fields to the transcription worker task state
- [x] 2.2 After successful transcription of a segment, update the cached text for that segment's source device
- [x] 2.3 Before transcribing a new segment, pass the cached previous text as `initial_prompt` to `WhisperEngine::transcribe_audio_with_confidence()`
- [x] 2.4 Clear cached previous-text state when `reset_speech_detected_flag()` is called (new recording session)

## 3. Whisper engine prompt support

- [x] 3.1 Add `initial_prompt: Option<String>` parameter to `WhisperEngine::transcribe_audio_with_confidence()` signature
- [x] 3.2 If `initial_prompt` is `Some`, set `params.set_initial_prompt(initial_prompt)` before `state.full()`
- [x] 3.3 Ensure `condition_on_previous_text` is set to `true` in `FullParams` when prompt is provided

## 4. Tests

- [x] 4.1 Add unit test for segment accumulation: two segments with 300ms gap are merged into one chunk
- [x] 4.2 Add unit test for segment accumulation: two segments with 1000ms gap remain separate
- [x] 4.3 Run `cargo test -- audio::vad::tests` — all tests must pass
- [x] 4.4 Run `cargo test -- audio` — no regressions
