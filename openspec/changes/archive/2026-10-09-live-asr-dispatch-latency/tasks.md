# Tasks

Run Rust commands from the repo root. "Full Rust tests" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`, the same skips earlier changes used. Frontend tests: `cd frontend && bun test tests/`.

## 0. Preconditions and baseline

- [x] 0.1 Re-verify the anchors design.md (Context, D1–D5) relies on and note any drift here. Verify: each still matches.
  - `pipeline.rs`: gap flush (`>= 500.0`) and cap flush (`> 25_000.0`) after `vad.process_audio`; `flush_pending_segments` uses `merge_segments(pending, 500.0, 25 * 16000)`; `remap_segment_times_to_real`; `set_pending` / `set_pending_triggers(500, 25_000)`.
  - `vad.rs`: `processed_ms()`, `is_in_speech()`, `speech_start_sample` (padded start, 16 kHz samples).
  - `telemetry.rs`: `PendingState { segments, buffered_ms, gap_trigger_ms, cap_trigger_ms }`.
  - `RecordingControls.tsx`: status lines gated on `diarization.mode !== 'off'`.

  Note (2026-10-09): all anchors match; no drift. The gap rule only looks at the first new segment of a dispatch, but `flush_pending_segments` regroups with `merge_segments(.., 500.0, ..)`, so grouping does not depend on flush timing.
- [x] 0.2 Record baselines here: full Rust tests (pass/fail/ignored), `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^frontend.*warning:"`, and `bun test tests/` (pass/fail). Verify: the numbers are written down; group 5 compares against them.

  Baseline (2026-10-09, before any change): full Rust tests 639 passed / 0 failed / 9 ignored; clippy 32 warnings; `bun test tests/` 128 pass / 0 fail.

## 1. VAD in-progress speech accessors

- [x] 1.1 Add `current_speech_start_ms() -> Option<f64>` and `current_speech_ms() -> f64` to `ContinuousVadProcessor` (design D2), read-only. Verify: unit test in `vad.rs` feeds speech then silence and asserts `None`/0 before speech, a start and growing length during speech, and `None`/0 after the segment closes; `cargo test -p meetily --lib vad` passes.

## 2. Dispatch on silence

- [x] 2.1 Track a per-channel pending tail in VAD-counter time, set whenever segments are appended to pending and cleared on flush (design D1, "Clock"). Verify: `cargo check -p meetily` succeeds.

  Note (2026-10-09): superseded during 5.3. The decision now runs in recording time, so the tail is the last pending segment's stored end and no separate field is kept (design D1).
- [x] 2.2 After each VAD dispatch, flush the channel's pending speech when `horizon - tail >= 500 ms` (design D1), keeping the existing gap, cap and stop triggers. Verify: `cargo check -p meetily` succeeds.
- [x] 2.3 Pipeline tests, one per scenario of the `live-segment-merging` delta. Verify: `cargo test -p meetily --lib pipeline` passes with:
  - speaker stops talking: one phrase followed by silence is dispatched within one 200 ms window after tail + 500 ms, before any later speech and before stop;
  - silence shorter than the gap: two phrases 300 ms apart are dispatched as one chunk;
  - new speech ≥ 500 ms later: the first phrase is dispatched when the new speech starts, not when it ends;
  - 25 s cap still dispatches during continuous speech;
  - chunk boundaries unchanged: the same audio yields identical chunk sample vectors with and without the silence rule (run the old rule by disabling the check in the test helper);
  - a non-zero anchor shift (dropped samples) does not cause an early or late flush.

  Note (2026-10-09): Silero does not detect synthetic tones, so the tests use a TTS phrase fixture (design D6). The silent branch waits 500 + 150 ms pre-pad (design D1) so chunks stay identical; 6 tests, 3 repeated runs green.

## 3. Buffer telemetry

- [x] 3.1 Add `open_speech_ms` and `silence_ms` to `PendingState` and set them next to `set_pending` (design D3); add both to the `PendingState` type in `frontend/src/lib/ipc/recording.ts`. Verify: a `telemetry.rs` unit test sets them and reads them back from `snapshot()`; `cargo test -p meetily --lib telemetry` passes; `bunx tsc --noEmit` in `frontend` succeeds.

  Note (2026-10-09): `silence_ms` is the gap the dispatch rule measures (processed − pre-pad − tail, clamped at 0), so the ring reaches 500 ms exactly when the flush fires; set through a new `set_pending_progress` so `set_pending` callers are unchanged.

## 4. Indicator (variant A) and visibility

- [x] 4.1 Add `buildBufferTank(pending)` to `diarization-status-lines.ts` (design D4). Verify: `tests/lib/diarization-status-lines.test.ts` gains cases for idle (empty, no alert), speaking (open fraction grows), pending + silence (ring = silence/500, clamped), dispatched (empty), near cap (fraction against 25 s, clamped to 1), and stuck (alert only past 500 + 350 ms); `bun test tests/` passes.
- [x] 4.2 Render the tank and the 16 px ring in `ChannelRow` of `DiarizationStatusLines.tsx` next to the level bar, colours as in design D4, label in `title`. Verify: run the app (`/run` or `pnpm tauri dev`), record, and see the tank fill while speaking, the ring fill in pauses, both empty after dispatch.

  Verified (2026-10-09, user, recording `Meeting 2026-10-09_13-36`): the tank fills while speaking, the ring fills in pauses, both empty after dispatch; no stuck state seen.

  Status (2026-10-09): implemented (12 px ring, see design D4); tsc and `bun test tests/` pass. Open: the in-app check while recording.
- [x] 4.3 Show the status lines for every recording in `RecordingControls.tsx` (design D5). Verify: with diarization Off, the two lines and the model row appear during recording and the diarization indicator is idle (grey), not red; nothing is shown when not recording.

  Verified (2026-10-09, user): with diarization Off the two lines and the model row are shown and DIAR is grey.

  Status (2026-10-09): implemented (gate on a running pipeline, diarization text hidden when off, design D5); tsc passes. Open: the in-app check with diarization Off.
- [x] 4.4 Update `docs/CODEBASE_MAP_ARCHITECTURE.md` (live pipeline paragraph) to say pending speech is dispatched after 500 ms of silence. Verify: the paragraph names the silence trigger alongside the gap and 25 s triggers.

  Note (2026-10-09): the doc had no live-pipeline paragraph; the triggers were added to the Audio Pipeline row of the layer table.

## 5. Integration checks

- [x] 5.1 Full Rust tests, clippy warning count and `bun test tests/` match or improve on the 0.2 baselines. Verify: numbers recorded here.

  Result (2026-10-09): full Rust tests 648 passed / 0 failed / 10 ignored (baseline 639/0/9: 9 new tests, one of them the `#[ignore]` real-recording replay); clippy 31 warnings (baseline 32: the removed `drop(pending_ref)` in the cap path; no new ones); `bun test tests/` 134 pass / 0 fail (baseline 128).
- [x] 5.2 Record a real meeting of ≥ 10 minutes (mic + system) and measure display lag from the saved `transcripts.json` with the proposal's `display_time` method. Verify: p50 lag after phrase end ≤ ~1.5 s and no row lagging > 10 s while its channel is silent; numbers recorded here next to the baseline (p50 4.4 s, p90 20 s, max 214 s).

  Result (2026-10-09, `Meeting 2026-10-09_13-36`, 3 min, 22 rows; the user accepted the shorter recording): lag after phrase end p50 0.8 s, p90 1.7 s (baseline 4.4 / 20 s). Lag no longer depends on when the next phrase on the channel ends (median 1.0 s vs 0.8 s). The three rows at 12–25 s are the first halves of 25 s cap splits during continuous speech (shown together with their second halves), not waits during silence; without them the maximum is 1.7 s.
- [x] 5.3 Confirm transcript quality did not change on real audio: an `#[ignore]` pipeline test replays a recording given by `MEETILY_REPLAY_AUDIO` (stereo, L = mic, R = system) through the live pipeline with and without dispatch on silence. Verify: `MEETILY_REPLAY_AUDIO=transcript_compare/2026-10-08_10-00/audio.mp4 cargo test -p meetily --lib replay_recording -- --ignored` passes with identical chunk samples per channel; chunk counts recorded here.

  Note (2026-10-09): replaces the earlier `asr-eval` replay. `asr-eval` re-implements the chunking instead of running `pipeline.rs`, so it cannot see this change.

  Result (2026-10-09): identical chunk samples with and without dispatch on silence on both recordings: `2026-10-08_10-00` (30 min): mic 66 / sys 191; `2026-10-07_13-00` (99 min): mic 40 / sys 777. The first attempt, deciding in VAD-counter time, gave sys 195 vs 191 on 10-08 and led to design D1's recording-time check.
- [x] 5.4 Run `graphify update .` after the code changes. Verify: command succeeds.
