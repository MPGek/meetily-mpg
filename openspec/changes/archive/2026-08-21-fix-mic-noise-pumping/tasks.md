## 1. Remove live AGC from the microphone chain

- [x] 1.1 Remove the STEP 3 `LoudnessNormalizer` call from the mic enhancement block in `audio/pipeline.rs` (~lines 523-536) and update the chain comment to "high-pass → noise suppression"
- [x] 1.2 Remove the now-unused `LoudnessNormalizer`, `normalize_v2`, and private `TruePeakLimiter` from `audio/audio_processing.rs` (verified: no other callers exist)
- [x] 1.3 Remove the `ebur128` dependency from `frontend/src-tauri/Cargo.toml`
- [x] 1.4 Clean up now-unused imports in `audio/pipeline.rs` and `audio/audio_processing.rs` after the removals
- [x] 1.5 Run `cargo check` on `frontend/src-tauri` and confirm the workspace compiles with no warnings from the removals

## 2. VAD-driven ducking decision

- [x] 2.1 Change `AudioMixer::mix` in `audio/ffmpeg_mixer.rs` to decide ducking from a microphone speech-active flag instead of computing `mic_rms > 0.01` on the processed mic signal
- [x] 2.2 Add debounce state to the mixer: engage ducking immediately on speech, hold it for ~500 ms after speech ends, then release to full level
- [x] 2.3 Transition smoothly between ducked (0.6) and full (1.0) system gain rather than hard-switching
- [x] 2.4 Thread the speech-active flag through `FFmpegAudioMixer`/`pop_mixed` (accept the flag at the mix boundary; derive it from the per-channel Silero VAD in `AudioPipeline` where the mixer is driven)
- [x] 2.5 Add unit tests in `ffmpeg_mixer.rs` covering: no speech → system audio at full level; speech → ducked; speech ends + debounce → returns to full; loud non-speech noise without speech → not ducked
- [x] 2.6 Confirm existing `ffmpeg_mixer.rs` tests still pass

## 3. Neutralize dormant automatic-gain code

- [x] 3.1 Make `audio_v2::normalizer::AudioNormalizer::normalize` a passthrough (return an unchanged copy), removing the per-chunk peak-normalizing body and its stale EBU R128 TODO references
- [x] 3.2 Grep the crate for any remaining references to `LoudnessNormalizer`, `normalize_v2`, or the audio_processing `TruePeakLimiter` (outside documentation of removed code) and confirm none remain

## 4. Verification

- [x] 4.1 Run the full `cargo test` for `frontend/src-tauri` (or workspace) and confirm all tests pass
- [x] 4.2 Manual capture check: record ~30-60 s with only ambient noise on the mic and confirm the left channel's RMS/peak stays at its natural level with no upward ramping (use the existing diagnostic logging); confirm the right (system) channel is unaffected
- [x] 4.3 Run `cargo clippy` (if configured for this workspace) and confirm no new warnings from these changes