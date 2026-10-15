## 1. Encoder input sanitization

- [x] 1.1 In `frontend/src-tauri/src/audio/encode.rs`, add a sanitization step inside `encode_single_audio` that replaces every non-finite f32 sample (`is_nan()` / `is_infinite()`) with 0.0 before the data is cast to bytes and fed to ffmpeg
- [x] 1.2 Add a debug/trace log of how many samples were replaced when non-finite samples are found
- [x] 1.3 Add a unit test that encodes a buffer containing NaN and ±Inf samples and asserts the output is a valid identically-sized file (and, where cheaply verifiable, that the encode does not fail)
- [x] 1.4 Verify: `cargo test -p meetily` (or repo test command) passes, including existing `encode` and `incremental_saver` tests

## 2. Checkpoint failure drop-and-continue

- [x] 2.1 In `frontend/src-tauri/src/audio/incremental_saver.rs` `save_checkpoint`, on encode failure: log the error, increment a new `failed_checkpoints` counter, clear `checkpoint_buffer`, and return `Ok(())` so `add_chunk` keeps processing subsequent audio
- [x] 2.2 Ensure `save_checkpoint` never leaves stale buffered audio that would be re-encoded in a later call (buffer cleared on both success and failure)
- [x] 2.3 Add a unit test that forces a failed checkpoint encode and asserts: (a) checkpointing continues, (b) later audio still produces checkpoints, (c) the failed count is reported
- [x] 2.4 Verify: `cargo test` passes and a simulated poisoned chunk no longer wedges the accumulation task

## 3. Time-bounded ffmpeg operations

- [x] 3.1 In `encode_single_audio`, wrap the write-and-wait sequence so spawn/write/wait cannot block forever: if not completed within the checkpoint timeout (~60 s), kill the child and return a timed-out error handled per task 2.1
- [x] 3.2 In `incremental_saver.rs::merge_checkpoints` (and the recovery merge in the same file), apply the same bounded wait (~2 min) so a hung merge abandons the process and reports failure instead of hanging `finalize`
- [x] 3.3 Add a regression test (or a documented manual test step) that a deliberately hanging ffmpeg child is terminated and saving continues
- [x] 3.4 Verify: `cargo test` passes; no ffmpeg child processes remain after a timed-out operation

## 4. Checkpoint interval accounting

- [x] 4.1 In `incremental_saver.rs`, change the trigger threshold to be frame-based: compare accumulated audio frames (`sum(len) / channels`) against `sample_rate * 30` (or equivalently set the sample threshold to `sample_rate * 30 * channels` so interleaved stereo data triggers at ~30 s wall time)
- [x] 4.2 Fix the `duration_seconds` calculation in `save_checkpoint` logging so it reports the real duration instead of dividing the interleaved sample count by the sample rate
- [x] 4.3 Update existing checkpoint-count unit tests (`test_checkpoint_creation`) to expect ~30 s (i.e. half the previous checkpoint count for the same wall-clock test input)
- [x] 4.4 Verify: `cargo test` passes and a 60 s stereo test input produces ~2 checkpoints, not 4

## 5. Surface saver delivery failures in the pipeline

- [x] 5.1 In `frontend/src-tauri/src/audio/pipeline.rs`, replace `let _ = sender.send(recording_chunk)` with an explicit match that warns and reports (throttled) through `state.report_error(...)` when the saver channel is closed/unavailable
- [x] 5.2 Add a recoverable `AudioError` variant (e.g. `SaveUnavailable`) in `recording_state.rs` if needed for classification, ensuring it does not trip the automatic stop-recording threshold
- [x] 5.3 Verify: `cargo check`/`cargo test` passes; simulated closed saver channel logs a warning instead of failing silently

## 6. Partial-save reporting at finalization

- [x] 6.1 Expose `failed_checkpoints` from `IncrementalAudioSaver` through `RecorderSaver::get_stats()` so the stop flow can see it
- [x] 6.2 In `stop_and_save`, after a successful merge, probe the produced `audio.mp4` duration (bundled ffprobe/ffmpeg parse) and compare with the recorded session duration; treat a gap of >5% and >30 s as a significant mismatch
- [x] 6.3 When `failed_checkpoints > 0` or a significant duration mismatch exists, emit a new `recording-audio-warning` Tauri event (payload: saved vs expected duration, failed checkpoint count) and write an additive `audio_warning` field into `metadata.json` instead of an unqualified `completed`
- [x] 6.4 Ensure probe failure itself never fails the save (log and continue)
- [x] 6.5 Verify: `cargo test` passes; a short-audio scenario emits the warning and metadata flag

## 7. Frontend awareness of the partial-audio warning (minimal)

- [x] 7.1 In the frontend, listen for `recording-audio-warning` and surface it using the existing toast/notification patterns (reuse `recording-error` presentation path where practical)
- [x] 7.2 Do not change the transcript/meeting save flow — the warning is additive and display-only

## 8. End-to-end verification

- [x] 8.1 Run `cargo clippy` and the full Rust test suite; fix any warnings introduced
- [x] 8.2 Run a real ~3-4 minute stereo recording with the app (dev build) and confirm: audio.mp4 duration ≈ session duration, ~8 checkpoints, no warning emitted
- [x] 8.3 Simulate the original incident (inject NaN into the mic path or a forced checkpoint failure) and confirm the app now produces a short-silence region plus a `recording-audio-warning` instead of a silent half-length save
- [x] 8.4 Run `openspec validate` from the repo root and fix any spec issues

## 9. System-channel transcript timestamp anchoring (found during 8.2 manual test)

Manual verification with a ~45-minute recording showed the **system-channel** transcript timestamps lagging actual audio by ~12 minutes (a segment from the last seconds, real time 44:52, was stamped 32:30); microphone timestamps looked correct. Root cause: live transcription timestamps derive from the VAD's per-source sample counter; a system stream that starts late or delivers nothing during silent periods (WASAPI loopback) shifts every later system segment into the past by the accumulated gap.

- [x] 9.1 In `frontend/src-tauri/src/audio/vad.rs`, expose `ContinuousVadProcessor::processed_ms()` so the pipeline can map VAD sample-domain positions to real time
- [x] 9.2 In `frontend/src-tauri/src/audio/pipeline.rs`, anchor each source's VAD timeline to real capture time: record the `chunk.timestamp` (global recording clock) of the oldest sample of each dispatch buffer and push `(vad_counter_ms, real_seconds)` anchors per dispatch (including the final flush path)
- [x] 9.3 Remap every produced VAD segment (dispatch + `flush_remaining_audio` + `vad.flush()`) from the sample domain to recording-relative seconds via `remap_segment_times_to_real` (binary-search anchor lookup, clamped at 0)
- [x] 9.4 Unit tests for the remap function (late stream start, gap compression, empty anchors no-op, negative clamp)
- [x] 9.5 Verify: `cargo test -p meetily --lib -- audio::pipeline audio::vad audio::incremental_saver` passes and live system timestamps track the audio file