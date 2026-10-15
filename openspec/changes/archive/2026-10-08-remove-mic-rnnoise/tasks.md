# Tasks

Run Rust commands from `frontend/src-tauri`. "Full Rust tests" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`, the same skips earlier changes used.

## 0. Preconditions and baseline

- [x] 0.1 Re-verify the anchors that design.md (Context) cites and note any drift in this task: `AudioCapture` fields and the mic-only init block in `pipeline.rs`, the HPF → RNNoise steps in `process_audio_data`, `RNNOISE_APPLY_ENABLED` in `ffmpeg_mixer.rs` and its re-export in `audio/mod.rs`, and the single `nnnoiseless` / `realfft` users in `audio_processing.rs`. Verify: `grep -rn "nnnoiseless\|realfft\|RNNOISE_APPLY_ENABLED\|NoiseSuppressionProcessor\|spectral_subtraction\|average_noise_spectrum" src` lists no other users.
  - Note (2026-10-08): verified at 8ca3918, no drift. `pipeline.rs`: fields 347-348, init 405-443, `process_audio_data` STEP 1/2 651/658-693. `ffmpeg_mixer.rs:20`, `mod.rs:150`. The grep lists only `audio_processing.rs`, `pipeline.rs`, `ffmpeg_mixer.rs` and `mod.rs`.
- [x] 0.2 Record baselines in this task's note: full Rust test counts (pass/fail/ignored), and the clippy warning count from `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^src.*warning:"`. Verify: both numbers are written down. Group 3 compares against them.
  - Note (2026-10-08): full Rust tests 638 passed / 0 failed / 9 ignored. Clippy 32. Clippy prints paths as `frontend\src-tauri\src\...`, so the count uses `grep -c "^frontend.*warning:"`; the `^src` pattern in this task's text counts 0. None of the 32 warnings are in `audio_processing.rs`, `ffmpeg_mixer.rs` or `audio/mod.rs`. The `pipeline.rs` ones (339 `recording_sender`, 1281/1298 `drop`) are pre-existing.

## 1. Remove RNNoise from the mic chain (D1, D2)

- [x] 1.1 In `pipeline.rs`, remove the `noise_suppressor` field, its construction in `AudioCapture::new`, and STEP 2 in `process_audio_data`, including the health and length-mismatch logging. Fix the surrounding comments so they describe HPF → unity gain. Drop `NoiseSuppressionProcessor` from the `use` line. Verify: `cargo check -p meetily` succeeds.
  - Note: the ring buffer's max-size comment (`pipeline.rs:88`) also listed "RNNoise buffering" as a reason; that mention is removed. The buffer size is unchanged.
- [x] 1.2 Delete `RNNOISE_APPLY_ENABLED` and its doc comment from `ffmpeg_mixer.rs`, and drop it from the `pub use ffmpeg_mixer::{...}` in `audio/mod.rs`. Verify: `cargo check -p meetily` succeeds.
- [x] 1.3 Delete `NoiseSuppressionProcessor` and the `nnnoiseless` import from `audio_processing.rs`. Remove `nnnoiseless = "0.5"` from `Cargo.toml`, leaving the uncommitted `asr-eval` `[[bin]]` edit untouched. Verify: `cargo check -p meetily` succeeds, `git diff Cargo.toml` shows only the dependency line removed besides the pre-existing bin block, and `Cargo.lock` no longer lists `nnnoiseless`.

## 2. Remove dead noise-suppression code (D3)

- [x] 2.1 Delete `spectral_subtraction`, `average_noise_spectrum` and the `realfft` imports from `audio_processing.rs`, and remove `realfft = "3.4.0"` from `Cargo.toml`. Verify: `cargo check -p meetily` succeeds. `Cargo.lock` drops `realfft` only if no other crate depends on it.
  - Note: `realfft` 3.5.0 stays in `Cargo.lock` as a transitive dependency of `polyvoice` and `rubato`. The `warn` log import became unused and was removed.
- [x] 2.2 Remove `HighPassFilter::reset` and the unused `sample_rate` / `cutoff_hz` fields with their `#[allow(dead_code)]`, keeping the filter math and its init log unchanged. Add a unit test in `audio_processing.rs`: a 1 kHz sine at 48 kHz passes within ±0.5 dB RMS after a settling period, and a 20 Hz sine is attenuated by at least 10 dB. This pins the "Only low-frequency rumble is removed" scenario. Verify: `cargo test -p meetily --lib audio_processing` passes.
- [x] 2.3 Update `docs/CODEBASE_MAP.md` (Audio Capture line) and `docs/CODEBASE_MAP_ARCHITECTURE.md` (Audio Pipeline row and the "Noise Suppression | nnnoiseless" stack row) so they no longer list RNNoise. Verify: `grep -rni "rnnoise\|nnnoiseless" docs` returns nothing.

## 3. Integration checks

- [x] 3.1 Full Rust tests and clippy against the group 0 baselines. Verify: no new failures; the pass count goes up by the new HPF test; the clippy count is equal or lower. Record both in this task's note.
  - Note (2026-10-08): 639 passed / 0 failed / 9 ignored (baseline 638, +1 HPF test). Clippy 32, the same set of warnings as the baseline once line numbers are ignored.
- [x] 3.2 `grep -rni "rnnoise\|nnnoiseless\|noise_suppress\|NoiseSuppression" frontend/src-tauri/src docs` returns nothing outside comments that explain the removal. Verify: grep output recorded in this task's note.
  - Note (2026-10-08): no matches (grep exit 1) in `frontend/src-tauri/src` or `docs`.
- [ ] 3.3 Manual (live app): record 30-60 s with mic speech and some background noise. Verify: the log has the HPF init line and no RNNoise lines; the saved `audio.mp4` left channel plays normally and its speech is transcribed on the mic channel.
- [x] 3.4 Run `graphify update .` from the repo root. Verify: the command finishes without error.
