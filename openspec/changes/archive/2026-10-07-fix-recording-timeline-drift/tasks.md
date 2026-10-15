# Tasks

Run Rust commands from the repo root. "Full Rust tests" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`, the same skips earlier changes used. Saver tests that run ffmpeg need it on PATH, like the existing `test_checkpoint_creation`.

## 0. Preconditions and baseline

- [x] 0.1 Re-verify the anchors design.md (Context) cites and note any drift in this task. Verify: each one still matches:
  - `pipeline.rs`: `AudioMixerRingBuffer` (`add_samples` / `can_mix` / `extract_window`), `close_mic_gap` called before `add_samples` in `run`, `emit_ready_windows`, the run-loop exit
  - `incremental_saver.rs`: `save_checkpoint`, `merge_checkpoints`, `recover_audio_from_checkpoints`, `has_audio_checkpoints`
  - `encode.rs`: `encode_single_audio`, `ENCODE_TIMEOUT`, `MERGE_TIMEOUT`
  - `recording/stop.rs`: the fixed 300 s timeout around `save_recording_only`
  - Note (2026-10-05): verified at 3134b6b, no drift. `pipeline.rs`: `add_samples` 110, `can_mix` 153, `extract_window` 158, `close_mic_gap(&chunk)` 1085 before `add_samples` 1208, run-loop exit 1217-1232, `emit_ready_windows` 1240. `incremental_saver.rs`: 91 / 181 / 322 / 508. `encode.rs`: `ENCODE_TIMEOUT` 27, `MERGE_TIMEOUT` 29, `encode_single_audio` 111. `stop.rs:394` (300 s).
- [x] 0.2 Record baselines in this task's note:
  - full Rust tests (pass/fail/ignored)
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^frontend.*warning:"`

  Verify: both numbers are written down. Group 8 compares against them.
  - Note (2026-10-05): full Rust tests 621 passed / 0 failed / 9 ignored. Clippy 32. Pre-existing ones in touched files: `pipeline.rs:234` (unused `recording_sender`) and `pipeline.rs:1170,1187` (`drop` of a reference). The comparison runs in group 9, not "Group 8" as the task text says.

## 1. Mixer: cut only when both sides are full, pad only a starved side (D1)

- [x] 1.1 Change `AudioMixerRingBuffer` so `can_mix` is `(mic ≥ W && sys ≥ W) || max ≥ W + SLACK`, with `SLACK = W`. A side that is short at a cut is drained, zero-padded as today and marked starved (one flag per side). Mic padding is still counted through `count_mic_padding`. Add a throttled info log (once per 10 s) on starvation cuts, with the side and the lead in ms. Verify: `cargo check -p meetily` succeeds.
- [x] 1.2 Unit tests in `pipeline.rs` (pure, no devices). Verify: `cargo test -p meetily --lib pipeline` passes with:
  - both sides fed for 1600 s in 480- and 960-sample chunks with up to 20 ms of relative jitter: emitted frames + buffered frames equal fed frames on each side, and no padded zero was emitted
  - only the system side fed: windows are emitted once it is W + SLACK ahead, and the mic side is all zeros
  - a 100 ms burst pattern on one side does not trigger starvation
- [x] 1.3 Update `ring_buffer_counts_mic_padding_only_while_a_gap_is_open` to the new rule: system-only windows need W + SLACK buffered before a cut. The assertions about which padding is counted stay the same. Verify: the test passes.

## 2. Resume alignment of a starved side (D2)

- [x] 2.1 Add an alignment step on the ring buffer, called with the incoming chunk length before the chunk is added. For a starved side it pushes `other_len − own_len − c` zeros (saturating), counts them through `count_mic_padding` on the mic side, and clears the flag. Wire it into `run` for both device types before `add_samples` (the mic ordering is finished in 3.1). Verify: `cargo check -p meetily`.
- [x] 2.2 Unit tests. Verify: `cargo test -p meetily --lib pipeline` passes with:
  - system side silent for 3 s every 20 s, and for one 60 s span, then resuming: a resumed system sample lands within one chunk of the mic sample fed at the same step
  - no alignment zeros when the side was never starved
  - alignment zeros on the mic side are counted while a mic gap is open

## 3. Mic gap fill ordering and dead band (D3)

- [x] 3.1 In `run`, for a mic chunk, do the D2 alignment first, then `close_mic_gap`, then the rest unchanged. Give `mic_gap_fill_samples` a dead band: a remainder below one window returns 0, a larger one is filled as today (30 s cap kept). Count mic-side zeros since the last real mic chunk and seed the gap count with them in `begin_mic_gap` (design D3, "Seed the gap count"). Verify: `cargo check -p meetily`.
- [x] 3.2 Update `mic_gap_fill_is_the_remainder_after_partial_coverage` (a 400 ms remainder now returns 0) and add a case just above one window that is still filled. Add pipeline-level tests:
  - mic gap with system audio flowing: total mic zeros equal the gap within one chunk, and no timestamp fill is added
  - mic gap with system audio silent: the fill equals the gap
  - `mic_gap_counts_padding_from_the_last_mic_chunk`: marker 4 s after the mic stopped; post-gap mic lines up with system audio within one chunk. It fails on HEAD with a 3.9 s shift.

  Verify: `cargo test -p meetily --lib pipeline` passes, including the existing `mic_gap_fill_*` tests.

## 4. Save the ring-buffer tail at pipeline end (D4)

- [x] 4.1 After the run loop exits, emit one final chunk of `max(mic_len, sys_len)` frames, with the shorter side zero-padded, through the same `try_send` path, if either side holds samples. Verify: `cargo check -p meetily`.
- [x] 4.2 Test with the existing `run_pipeline` helper: feed 1.4 s on both sides, close the channel, and assert the saver received 1.4 s of stereo frames, not 1.2 s. Verify: `cargo test -p meetily --lib pipeline` passes.
- [x] 4.3 Update the stale "Ring Buffer Mixing" paragraph in `docs/PROJECT_OVERVIEW_FULL.md` to the current rule: 600 ms windows, cut when both are full, starved-side padding, resume alignment, no ducking on the live path. Verify: the paragraph matches design D1-D4.

## 5. Lossless FLAC checkpoints (D5)

- [x] 5.1 Add a FLAC checkpoint encoder in `encode.rs` next to `encode_single_audio`:
  - `-f f32le … -c:a flac -sample_fmt s32`
  - the same non-finite sanitization, `run_ffmpeg_with_timeout` and `ENCODE_TIMEOUT`
  - factor out the shared spawn code if that is less code than duplicating it

  `save_checkpoint` writes `audio_chunk_NNN.flac` with it. `encode_single_audio` stays AAC for `audio_processing.rs`. Verify: `cargo check -p meetily`.
- [x] 5.2 Tests in `encode.rs`:
  - a FLAC checkpoint with NaN/Inf samples encodes to a valid file
  - `ffprobe` reports 2 channels, 48 kHz and exactly the input frame count

  `test_checkpoint_creation` and `test_checkpoint_failure_drop_and_continue` pass unchanged apart from the file extension. Verify: `cargo test -p meetily --lib encode incremental_saver`.

## 6. Merge: concat FLAC → one AAC encode (D6)

- [x] 6.1 Change `merge_checkpoints` to `-f concat -safe 0 -i list` over the `.flac` files, with the AAC codec arguments taken from the existing `encode.rs` constants (no duplicated literals). Keep the error, verify and cleanup behaviour. Verify: `cargo check -p meetily`.
- [x] 6.2 Integration test in `incremental_saver.rs`: feed 75 s of stereo with a 1 ms click every second on both channels across 3 checkpoints, finalize, decode `audio.mp4`, and assert:
  - every click is within 1024 samples of its fed position
  - the decoded length is within 1024 frames of the fed length
  - the file is AAC-LC, 2 channels, 48 kHz

  Verify: the test passes. On the old merge it would show +37 ms per boundary.

## 7. Timeouts that scale with length (D7)

- [x] 7.1 Add `merge_timeout(audio_secs) = 120 s + audio_secs / 8` and use it in `merge_checkpoints`. The saver computes `audio_secs` from the frames it wrote plus the buffered remainder, and exposes its finalize budget. Unit test the function: 0 s gives 120 s, and 3 h gives 1470 s. Verify: `cargo test -p meetily --lib incremental_saver`.
- [x] 7.2 In `recording/stop.rs`, replace the fixed 300 s around `save_recording_only` with the saver's finalize budget + 60 s, reached through the manager without holding a lock across an await. Verify: `cargo check -p meetily`. A test or comment shows that a 3 h budget exceeds 300 s.
  - Note (2026-10-05): the timeout is `max(300 s, budget + 60 s)`, so short recordings keep the former 5 minutes. `RecordingManager::finalize_budget` returns a `'static` future that clones the saver `Arc`, because `RecordingManager` is not `Sync` and a `&self` held across the await broke the `Send` bound of `stop_recording`. The test `save_timeout_covers_the_merge_of_a_long_recording` gives 1590 s for 3 h.

## 8. Recovery for both formats (D8)

- [x] 8.1 `recover_audio_from_checkpoints`:
  - collects `audio_chunk_<n>.<ext>` and sorts by numeric `n`
  - with any `.flac`, runs the D6 encode under `merge_timeout(count × 30 s)`
  - otherwise, runs the existing `.mp4` copy merge

  `has_audio_checkpoints` accepts `.flac` or `.mp4`. `AudioRecoveryStatus` is unchanged. Verify: `cargo check -p meetily`.
- [x] 8.2 Tests:
  - a FLAC checkpoint folder recovers with the click positions of 6.2
  - a folder of legacy AAC `.mp4` checkpoints still recovers
  - numeric ordering puts index 1000 after 101
  - `has_audio_checkpoints` is true for either extension and false for an empty folder

  Verify: `cargo test -p meetily --lib incremental_saver`.

## 9. Integration checks

- [x] 9.1 Full Rust tests and clippy compared with the 0.2 baselines. Verify: no new failures, and no new clippy warnings in touched files.
  - Note (2026-10-05): 638 passed / 0 failed / 9 ignored (baseline 621 + 17 new). Clippy 32, the same as the baseline. In touched files only the 3 pre-existing `pipeline.rs` lints remain; one new MSRV lint (`iter::repeat_n` is 1.82, the MSRV is 1.80) was fixed. My hunks are rustfmt-clean; `pipeline.rs`, `recording_manager.rs` and `recording_saver.rs` keep the formatting drift they already had at HEAD.
- [ ] 9.2 Manual: record at least 20 minutes of a call with both channels. Verify:
  - the `ffprobe` duration of `audio.mp4` is within 0.5 s of the session length (`created_at` to stop)
  - ▷ on a block after minute 20 plays that block's own speech
  - the finalize time is noted in this task
- [ ] 9.3 Manual: during a recording, leave system output silent for at least 30 s, then play a sound that the microphone also picks up. Verify: in the saved file the sound sits at the same position on both channels within 50 ms (inspect the waveform, or use the click check from 6.2).
- [x] 9.4 Run `graphify update .`. Verify: the command finishes without errors.
