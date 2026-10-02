# Tasks

Run Rust commands from the repo root. Run frontend commands from `frontend/`. "Full Rust tests" means `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands`, the prescribed skips used by change 07.

## 0. Preconditions and baseline

- [x] 0.1 Re-verify the anchors this plan cites. Verify: each of the following still matches design.md (Context, D11), and any drift is noted in this task:
  - `grep -rn "poll_audio_device_events\|get_reconnection_status\|attempt_device_reconnect" frontend/src` is empty
  - `grep -n "poll_audio_device_events\|get_reconnection_status\|attempt_device_reconnect" frontend/src-tauri/src/lib.rs` shows the 3 registrations
  - `recording_manager.rs:44,144-151` (monitor created and started) and `:546-682` (reconnect methods)
  - `device_monitor.rs:241` (fire once at `==` threshold)
  - `stop.rs:43-46` (Stop's first mutation is `take()`)
  - `stream.rs:42-47` (4-argument `AudioStream::create`)
  - Note (2026-10-02): verified at d78ecf7. Frontend grep is empty; `stop.rs:43-46`, `stream.rs:42-47`, `device_monitor.rs:241`, `recording_manager.rs:44` and the `lifecycle.rs`/`recording_commands.rs`/`recording_state.rs` anchors still match. Drift from the three changes landed since f9919e4 (`port-upstream-041-quick-fixes`, `harden-model-downloads`, `summary-run-integrity`): `lib.rs` registrations are at 707-709 (design said 705-708; the comment line is 706); `recording_manager.rs` monitor start is at 149-156 and the reconnect methods at 551-687 (+5, because `start_recording` now resets state when `pipeline_manager.start` fails, 121-137); `pipeline.rs` anchors moved by about +4 (flush check 964-974, STEP 3 1136-1173, force flush 1395-1459) and `AudioPipeline::new` now returns `Result` with the audio sender published after it (`pipeline.rs:1340-1355`). Both landed behaviors are kept unchanged by this change.
- [x] 0.2 Record the baselines in this task's note:
  - full Rust tests (pass/fail/ignored counts)
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^warning"`
  - `bun test tests/` and `pnpm exec tsc --noEmit -p .`

  Verify: the four numbers or outcomes are written down. Group 9 compares against them.
  - Note (2026-10-02): full Rust tests 592 passed / 0 failed / 9 ignored. Clippy: the literal `grep -c "^warning"` counts 18 lines (build-script notes and summaries, not lints, because `--message-format=short` prefixes lints with the path); counted per location (`grep -c "^frontend.*warning:"`) it is 32, which is the number group 9 compares. Pre-existing lints already in files this change touches: `pipeline.rs:177` (unused `recording_sender`), `pipeline.rs:1092,1109` (`drop` of a reference), `recording_state.rs:127` (complex type), `recording_commands.rs:673` (guard across await in a test). `bun test tests/` 117 pass / 0 fail; `pnpm exec tsc --noEmit -p .` clean.

## 1. Remove the dead reconnect surface

- [x] 1.1 Delete the commands `poll_audio_device_events`, `get_reconnection_status` and `attempt_device_reconnect`, with `DeviceEventResponse`, `ReconnectionStatus` and `DisconnectedDeviceInfo` (`audio/recording_commands.rs`). Keep `get_active_audio_output`. Remove their 3 registrations and the comment line in `lib.rs` `generate_handler!`. Drop the `DeviceEvent`/`DeviceMonitorType` imports in `recording_commands.rs` if they become unused. Verify: `cargo check -p meetily` succeeds.
  - Note (2026-10-02): removed as listed, plus the now-unused `error`/`warn` log imports; the section header above `get_active_audio_output` now reads "PLAYBACK DEVICE COMMANDS". The 3 registrations sat at `lib.rs:707-709` (drift noted in 0.1).
- [x] 1.2 Delete `RecordingManager::{poll_device_events, attempt_device_reconnect, handle_device_disconnect, handle_device_reconnect, is_reconnecting}`. Delete `RecordingState`'s `is_reconnecting` / `disconnected_device` fields, their accessors, and the init/cleanup lines that touch them (design D11). Remove imports that become unused (`list_audio_devices`, `RecordingDeviceType`, `DeviceMonitorType` in `recording_manager.rs`). Verify: `cargo check -p meetily`, and `grep -rn "is_reconnecting\|disconnected_device\|poll_device_events\|handle_device_reconnect" frontend/src-tauri/src --include=*.rs` returns only `recording_commands.rs.backup` hits, if any.
  - Note (2026-10-02): `cargo check` is clean apart from one expected intermediate warning, `device_event_receiver` never read, which task 4.1's `take_device_event_receiver` resolves. The grep returns nothing (no `.backup` file exists).
- [x] 1.3 Reword the doc comment of `take_drop_await_restore_does_not_hold_the_lock_across_the_await` (`recording_commands.rs` tests) so it names the mic-swap phases instead of `attempt_device_reconnect`. Leave the test body unchanged. Verify: `cargo test -p meetily --lib recording_commands` passes with the same 4 tests.
  - Note (2026-10-02): the inline comment inside the test that also named `attempt_device_reconnect` was reworded too (comment only, no code change), so no reference to the removed command remains. 4 tests pass.

## 2. Device monitor: re-fire for the mic and a retarget mailbox

- [x] 2.1 Extract the per-cycle decision for one device into a pure `MonitoredDevice::observe(&mut self, present: bool) -> Option<Observation>`, where `Observation` is `Disconnected` or `Reconnected`. Microphone entries report `Disconnected` every time `consecutive_missing % threshold == 0`. System-audio entries keep fire-once at `== threshold`. `monitor_loop` maps observations to `DeviceEvent`s. Verify with new unit tests in `device_monitor.rs`:
  - a wired mic missing for 2, 4 and 6 cycles yields 3 `Disconnected`
  - a system device missing for 6 cycles yields 1
  - present-after-missing yields `Reconnected` once and resets the counter
  - Note (2026-10-02): the loop keeps its existing log lines; the `Disconnected` warning now also states the missing count so re-fires are distinguishable in a support log. Tests: `wired_mic_re_reports_disconnect_every_threshold_cycles` (fires at cycles 2, 4, 6), `system_device_reports_disconnect_once`, `present_after_missing_reports_reconnected_once_and_resets`.
- [x] 2.2 Add `retarget: Arc<std::sync::Mutex<Option<String>>>` and `pub fn notify_mic_swapped(&self, name: String)` to `AudioDeviceMonitor`. At the top of each cycle the loop `take()`s the mailbox (`lock_or_recover`) and replaces the microphone entry with `MonitoredDevice::new(name, Microphone)` (design D5). Factor the replacement into a pure helper over `&mut Vec<MonitoredDevice>`. Verify with a unit test: after a retarget, the mic entry has the new name, `consecutive_missing == 0`, and the system entry is untouched. `cargo test -p meetily --lib device_monitor` passes.
  - Note (2026-10-02): the helper is `apply_mic_retarget(&mut Vec<MonitoredDevice>, String)`; it pushes a mic entry if none exists (not reachable today, since a swap only happens in a session that had a mic). Added a second test, `notify_mic_swapped_fills_the_mailbox`. `cargo test -p meetily --lib device_monitor`: 7 passed.

## 3. Pipeline continuity: discontinuity marker, mic VAD close, gap fill

- [x] 3.1 Add `pub const MIC_DISCONTINUITY_CHUNK_ID: u64 = u64::MAX - 20` (in `pipeline.rs`, outside the flush range `>= u64::MAX - 10`). Add `RecordingState::mark_mic_discontinuity(&self) -> bool`. It `try_send`s an empty Microphone marker chunk with that id even while paused, and returns whether it was delivered. Verify with a `recording_state.rs` unit test: with a capacity-4 sender installed and the state paused, the marker arrives with the reserved id, and a normal chunk sent while paused does not.
  - Note (2026-10-02): the marker carries the current recording duration as its timestamp (the pipeline uses it as the gap start only if no mic chunk was ever seen) and `sample_rate` 48000. A failed `try_send` logs `[HOT_SWAP] mic discontinuity marker not delivered`. Added a second test, `mic_discontinuity_marker_reports_no_pipeline`.
- [x] 3.2 Split `flush_remaining_audio` into a per-channel `flush_channel(DeviceType)` (dispatch the buffer with its anchor, `vad.flush()`, `flush_pending_segments`). `flush_remaining_audio` calls it for both channels. Factor STEP 3 of `run` into `emit_ready_windows(timestamp)`. Behavior must not change. Verify: the existing `pipeline.rs` tests pass, and a new test shows `ContinuousVadProcessor` keeps returning segments after a mid-stream `flush()` (speech, flush, speech → two separate segments).
  - Note (2026-10-02): `flush_remaining_audio` now runs each channel to completion in turn (mic: dispatch, `flush()`, send; then system) instead of both dispatches, both flushes, both sends. The segments sent are the same and mic segments still precede system ones in the transcription queue. The new test (`vad_processor_keeps_segmenting_after_a_mid_stream_flush`, in `pipeline.rs`) sets the VAD thresholds to 0 so every frame is speech, because Silero's score on a synthetic signal is not reliable.
- [x] 3.3 Add `mic_pad_since_gap: Option<usize>` to `AudioMixerRingBuffer`. While it is `Some`, `extract_window` adds every zero it pads onto the mic side (both the partial and the empty branch). Add `begin_mic_gap()`, which sets it only if it is unset, and `take_mic_gap_padding() -> usize`. Verify with a unit test: with only system samples for 3 windows after `begin_mic_gap()`, `take_mic_gap_padding()` returns `3 × window`; without a gap it counts nothing.
  - Note (2026-10-02): the test also checks a partial mic window (its padding counts) and that a second `begin_mic_gap()` keeps the count.
- [x] 3.4 Add the pure `mic_gap_fill_samples(gap_start, first_chunk_start, padded, sample_rate, cap_secs) -> usize` (design D7), returning `gap × rate − padded`, saturating at 0 and capped. Verify with unit tests for:
  - system audio covered the gap → 0
  - no system audio → the full gap
  - partial coverage → the remainder
  - a negative gap → 0
  - a gap over 30 s → the cap
- [x] 3.5 Handle the marker in `AudioPipeline::run`, before the flush check:
  - `flush_channel(Microphone)`
  - if no gap is open: `mic_gap_start = last_mic_chunk_end` (tracked from every mic chunk's timestamp) and `ring_buffer.begin_mic_gap()`
  - `continue`

  On the first mic chunk while a gap is open:
  - compute the fill with 3.4
  - push zeros into the mic side in window-sized steps, calling `emit_ready_windows` after each
  - log `[HOT_SWAP] mic gap: …`
  - close the gap, then process the chunk normally

  Verify with a `pipeline.rs` test that runs a real `AudioPipeline` (hold `TELEMETRY_TEST_LOCK`) with a recording receiver. Feed 2 s of mic chunks with consecutive timestamps, then the marker, then mic chunks stamped 5 s later and no system chunks. Assert that the interleaved left-channel samples emitted before the first post-gap sample total the elapsed recording time within one window. Run the same feed with continuous system chunks and assert no extra fill. `cargo test -p meetily --lib pipeline` passes.
  - Note (2026-10-02): both tests drive `AudioPipeline::run` on a local current-thread runtime while holding `TELEMETRY_TEST_LOCK`, so no guard is held across an `.await` (no new clippy lint), with 100 ms chunks. No system audio: the first post-gap mic frame lands at 7.0 s within one window. With system audio: the check is that mic and system post-gap frames are within one window of each other and of 7.0 s, not exact equality, because the ring buffer already zero-pads whichever side is short when the other completes a window (pre-existing behavior; with this in-phase synthetic feed each side shifts by less than one window). A double fill would put the mic about 4.6 s late and fails both checks. `cargo test -p meetily --lib pipeline`: 15 passed, 3 ignored (pre-existing ignores in other modules).

## 4. Mic recovery module and stream plumbing

- [x] 4.1 Add `AudioStreamManager::{take_mic_stream, set_mic_stream}` (`stream.rs`). Add `RecordingManager::{take_device_event_receiver, take_mic_stream_for_swap, install_swapped_mic(stream, device)}`. `install_swapped_mic` sets the stream, calls `state.set_microphone_device`, and calls `device_monitor.notify_mic_swapped(device.name)`. All synchronous. Verify: `cargo check -p meetily`.
  - Note (2026-10-02): also added `AudioStream::native_format() -> (rate, channels)`, recorded when the stream is created, so the D13 completion line can log the new device's native rate without a second enumeration. `install_swapped_mic` takes `Arc<AudioDevice>`. `cargo check` clean (the group 1 `device_event_receiver` warning is gone).
- [x] 4.2 Create `audio/recording/device_recovery.rs` and register it in `audio/recording/mod.rs`, with the sans-IO core:
  - `MicRecovery { current_mic, failed }` with `MAX_ATTEMPTS = 3`, `on_disconnect(name, live) -> Action` and `on_attempt_result(outcome, live) -> Option<UserEvent>` (design D4)
  - `install_decision(slot_state: Option<(&Arc<RecordingState>, bool)>, session: &Arc<RecordingState>) -> Install | Discard(reason)` (design D2)
  - `UserEvent` serializes to the D12 payloads

  Verify with unit tests built on real `RecordingState::new()` instances:
  - a stale session (slot holds another state) → `Discard(SessionChanged)`
  - Stop during the swap (slot `None`) → `Discard(SessionStopped)`
  - same session but not recording → `Discard(SessionStopped)`
  - same session, recording → `Install`
  - an event for a device other than `current_mic` → `Ignore(NotActiveMic)`
  - three failures → `MicSwapFailed{1}`, `MicSwapFailed{2}`, `MicRecoveryExhausted`, then `Ignore(Exhausted)`
  - an aborted (not-live) attempt → no event, budget unchanged
  - success → `MicDeviceSwitched`, budget reset, `current_mic` updated
  - payload JSON keys match D12
  - Note (2026-10-02): the outcome type is `AttemptOutcome { Switched { target }, Failed { error }, Aborted }`; `on_attempt_result` changes nothing and returns `None` both for `Aborted` and for any outcome when not live. Payloads are typed structs (`MicDeviceSwitchedPayload`, `MicSwapFailedPayload`, `MicRecoveryExhaustedPayload`, `SwitchReason` in snake_case). `UserEvent` carries `#[allow(clippy::enum_variant_names)]` because its `Mic*` variants mirror the event names. 9 tests, all on real `RecordingState::new()` instances where a state is needed.
- [x] 4.3 Implement `attempt_mic_fallback(app, session, lost_name)` with the settle/requery, Phase 1, blocking teardown with a 2 s cap, Phase 2 with one 500 ms retry, and Phase 3 (design D3). Each `RECORDING_MANAGER.lock_or_recover()` must be inside a block that ends before the next `.await`. Discarded streams are stopped after the lock is released. Log the `[HOT_SWAP]` lines from D13. Verify:
  - `cargo check -p meetily`
  - `cargo clippy -p meetily --all-targets` reports no `await_holding_lock` for this file
  - reading the function shows no guard alive across an `.await`; note that in this task
  - Note (2026-10-02): read-through: every `RECORDING_MANAGER.lock_or_recover()` is in a block (Phase 1, Phase 3) or in the sync `session_live` helper, and each ends before the next `.await`; the early `return Aborted` in Phase 1 happens inside the lock block with no await. The default-input query runs on `spawn_blocking`. A discarded replacement is stopped after the Phase 3 block releases the lock, with the same 2 s cap as the old stream's teardown. Clippy reports no `await_holding_lock` in `device_recovery.rs`.
- [x] 4.4 Implement `spawn_device_event_processor(app, receiver, session)`. One sequential loop: `DeviceDisconnected { Microphone }` → `MicRecovery::on_disconnect` → optional `attempt_mic_fallback` → `on_attempt_result` → emit the `UserEvent` only if `session_live` still holds. Other events are logged under `[DEVICE_EVENTS]`. The loop ends when the channel closes. Verify: `cargo test -p meetily --lib device_recovery` passes, and `cargo check -p meetily` reports no unused items in `device_recovery.rs`.
  - Note (2026-10-02): `spawn_device_event_processor` is `pub` (like the lifecycle entry points), so its call chain counts as used before group 5 wires it; the one remaining unused item until task 5.2 is `SwitchReason::UnavailableAtStart`. `DeviceListChanged` is logged at debug level (it fires on every device-count change); other non-mic events at info. `cargo test -p meetily --lib device_recovery`: 9 passed.
- [x] 4.5 Update `docs/CODEBASE_MAP_ARCHITECTURE.md`: the Recording row (`:125`) lists `device_recovery`, and the "Device monitoring uses mpsc channels…" line (`:297`) says the monitor feeds a per-session backend processor that falls back to the default mic. Verify: `bash scripts/check-doc-links.sh` reports no new broken links.
  - Note (2026-10-02): `bash scripts/check-doc-links.sh`: all references resolve.

## 5. Start-path wiring and start-time mic resolution

- [ ] 5.1 Add the pure `match_input_name(requested, enumerated: &[String]) -> Option<String>`. On Windows it accepts an exact or `contains` match, mirroring `get_windows_device`; elsewhere it requires an exact match. Add a thin `find_present_input(name)` over the platform's input enumeration. Change `resolve_microphone_device` to return `ResolvedMic { device /* enumerated name */, fell_back_from }` (design D6). Verify with unit tests in `recording/devices.rs`:
  - an exact match returns the enumerated name
  - on Windows, a substring match returns the full enumerated name
  - a missing name returns `None`
  - an empty list returns `None`
- [ ] 5.2 Replace the inline mic resolution in `start_recording_with_meeting_name` (`lifecycle.rs:90-135`) with `resolve_microphone_device(None, preferred_mic_name)`. Keep its existing error message text for "no microphone". In both start paths, after `start_recording` succeeds, emit `mic-device-switched { reason: "unavailable_at_start" }` when `fell_back_from` is set. Verify: `cargo check -p meetily`; `cargo test -p meetily --lib recording` passes.
- [ ] 5.3 In both start paths, call `manager.take_device_event_receiver()` and clone `manager.get_state()` before storing the manager. Spawn the processor after `IS_RECORDING.store(true)`. Verify:
  - `grep -n "spawn_device_event_processor" frontend/src-tauri/src/audio/recording/lifecycle.rs` shows exactly 2 call sites, each after its `IS_RECORDING.store(true`
  - `cargo check -p meetily` is clean of new warnings from groups 2-4 (all helpers are now used)

## 6. Frontend toasts

- [ ] 6.1 Add `MicDeviceSwitchedPayload`, `MicSwapFailedPayload` and `MicRecoveryExhaustedPayload` with `listenMicDeviceSwitched`, `listenMicSwapFailed` and `listenMicRecoveryExhausted` (via `listenTyped`) to `src/lib/ipc/recording.ts`. Add 1-to-1 `onMicDeviceSwitched`, `onMicSwapFailed` and `onMicRecoveryExhausted` wrappers to `src/services/recordingService.ts`. Verify with `tests/lib/ipc/recording-mic-events.test.ts`. It mocks both `@tauri-apps/api/core` and `@tauri-apps/api/event`, per the gotcha in `docs/CODEBASE_MAP_OPERATIONS.md`, and asserts each listener registers the exact event name.
- [ ] 6.2 Add the pure `micRecoveryToast(kind, payload)` in `src/lib/mic-recovery-toasts.ts` (design D12 copy):
  - switched → info naming the device; "for this meeting" for `disconnected`, "selected microphone unavailable" for `unavailable_at_start`
  - failed → warning with "retrying (n/3)"
  - exhausted → error saying the recording continues without a microphone, and to stop and restart

  Verify with `tests/lib/mic-recovery-toasts.test.ts`, covering all 4 variants with the device names in the text.
- [ ] 6.3 Add one mount-once effect to `RecordingStateProvider`. It registers the 3 listeners with a `cancelled` guard and an `isRecordingRef`, and shows `toast[level](title, { id: 'mic-recovery', description, duration })`. Suppress failed/exhausted toasts when not recording. Unlisten on cleanup. Verify: `bun test tests/` and `pnpm exec tsc --noEmit -p .` pass; `pnpm exec next lint` reports no new `no-restricted-imports` errors.

## 7. Integration checks (automated)

- [ ] 7.1 Run the full Rust tests and `bun test tests/`. Verify: pass counts equal the 0.2 baseline plus the tests added in groups 1-6, with no new failures.

## 8. Manual Windows verification (release or dev build, `%APPDATA%\com.meetily.ai\logs`)

- [ ] 8.1 USB mic unplug with system audio playing (a video). Start a recording on a USB mic, speak, unplug it, keep speaking into the laptop mic, then Stop. Verify:
  - within about 10 s the log has `[HOT_SWAP] mic '<usb>' disconnected → target '<laptop>'` and `[HOT_SWAP] mic switched to '<laptop>' (<rate> Hz…)`
  - a "Microphone switched" toast appears
  - mic transcript rows continue after the switch
  - at most 2 `Recoverable audio error` lines appear for the disconnect
  - Stop finishes normally
  - in the saved `audio.mp4` the left channel has the laptop-mic speech after the switch, aligned with the right channel at the same moment (check in the meeting audio player against transcript times)
- [ ] 8.2 Mic-only timeline, with no system audio playing. Repeat 8.1 in silence and speak a counted phrase ("one… two…") right after the switch. Verify:
  - the log shows `[HOT_SWAP] mic gap:` with a non-zero fill
  - the saved file's duration is within about 1 s of the session duration (no partial-audio warning)
  - clicking the post-switch transcript row in the audio player plays that phrase
- [ ] 8.3 Different sample rate. Start a recording with a Bluetooth headset as the mic (hands-free, typically 16 kHz), power the headset off, and let it fall back to the 48 kHz laptop array. If you can, also do the reverse: USB 48 kHz → BT 16 kHz as the next default. Verify:
  - the `[HOT_SWAP] … switched` line shows the new native rate
  - post-switch audio plays at normal speed and pitch
  - post-switch speech is transcribed
- [ ] 8.4 Stop during a swap. Unplug the mic, then press Stop 2-5 s later (around detection), 3 times. Verify:
  - Stop never hangs
  - no "Microphone fallback failed"/exhausted toast appears
  - the log shows either no swap or `[HOT_SWAP] discarded: session stopped`
  - the Windows mic privacy indicator turns off within about 1 s of Stop finishing
- [ ] 8.5 Stop→Start immediately. Unplug the mic, then Stop and Start a new recording right away (before about 4 s). Verify:
  - the new recording starts on the default mic
  - no `mic-device-switched` toast fires for the new session from the old disconnect
  - the new session's log has no `[HOT_SWAP]` attempt against the old device
- [ ] 8.6 Live diarization continuity. In Fast mode, rename a live mic speaker, unplug the mic, keep speaking, then Stop. Verify:
  - existing labels and the rename remain
  - post-switch mic rows get live labels
  - after Stop, mic rows from before and after the switch both have speaker labels

  Note whether the same person got a new cluster.
- [ ] 8.7 No fallback available. Disable all other input devices in Windows Sound settings, unplug the only mic, and keep recording for about 20 s. Verify:
  - 2 warning toasts ("retrying 1/3, 2/3") are each replaced by the next, then an error toast says recovery failed
  - the log shows `[HOT_SWAP] recovery exhausted`
  - system audio keeps recording
  - Stop works
  - a later new recording gets fresh attempts
- [ ] 8.8 Preferred mic missing at start. Set a preferred mic in settings, unplug it, and start a recording. Verify:
  - recording starts on the default
  - a "selected microphone unavailable" toast names the default
  - no `[HOT_SWAP]` attempt appears in the first 15 s

## 9. Final verification

- [ ] 9.1 Run the full Rust tests, `bun test tests/` and `pnpm exec tsc --noEmit -p .`. Verify: all pass; pass counts are the 0.2 baseline plus the new tests.
- [ ] 9.2 Run `cargo clippy -p meetily --all-targets --message-format=short`. Verify: the warning count is at most the 0.2 baseline, and no warning points at a file this change touched.
- [ ] 9.3 Run `openspec validate mic-hot-swap-recovery --strict`. Verify: it reports the change as valid.
- [ ] 9.4 Run `graphify update .`. Verify: it completes; `graphify query "mic hot swap device recovery"` surfaces `device_recovery.rs`.
