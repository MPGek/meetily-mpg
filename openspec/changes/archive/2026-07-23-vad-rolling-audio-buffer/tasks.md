## 1. Add rolling buffer to ContinuousVadProcessor

- [x] 1.1 Add `audio_history: VecDeque<f32>` field to `ContinuousVadProcessor` struct
- [x] 1.2 Add `buffer_capacity: usize` field to `ContinuousVadProcessor` struct (default: 5120 samples = 10 windows)
- [x] 1.3 Initialize the buffer in `ContinuousVadProcessor::new()` with the specified capacity
- [x] 1.4 Add `buffer_capacity: usize` field to `VadConfig` struct with default value of 5120

## 2. Update buffer after processing each window

- [x] 2.1 In `process_chunk()`, after processing each 512-sample window, append the window to `audio_history`
- [x] 2.2 If `audio_history.len()` exceeds `buffer_capacity`, remove the oldest samples to maintain fixed size
- [x] 2.3 Ensure buffer operations are efficient (O(1) amortized for VecDeque push_back/pop_front)

## 3. Use buffer for speech onset recovery

- [x] 3.1 In `process_chunk()`, when speech is detected (probability crosses positive threshold), modify the segment initialization logic
- [x] 3.2 Replace the current logic that adds zeros with logic that prepends `audio_history` to `current_speech`
- [x] 3.3 If `audio_history` is empty, start `current_speech` with only the current detection window (backward compatible)
- [x] 3.4 Update the `speech_start_sample` calculation to account for the prepended buffer audio

## 4. Tests

- [x] 4.1 Add unit test: buffer is initialized with correct capacity
- [x] 4.2 Add unit test: buffer is updated after processing windows
- [x] 4.3 Add unit test: buffer maintains fixed size (oldest samples removed when capacity exceeded)
- [x] 4.4 Add unit test: speech detection prepends buffer audio to segment
- [x] 4.5 Add unit test: speech detection with empty buffer works correctly (no backfill)
- [x] 4.6 Run `cargo test -- audio::vad::tests` — all tests must pass
- [x] 4.7 Run `cargo test -- audio` — no regressions
- [x] 4.8 Run integration test on real audio file to verify onset recovery
