# Proposal

## Why

`frontend/src-tauri/src/audio/recording_commands.rs:1742-1801` (`attempt_device_reconnect`) holds the global `std::sync::Mutex<Option<RecordingManager>>` guard across an `.await` (via `spawn_blocking` + `Handle::current().block_on`), so a slow device reconnect can make `stop_recording`, `poll_audio_device_events`, and every other command that locks `RECORDING_MANAGER` block for the full reconnect duration. More broadly, 86 `.lock().unwrap()` sites on global audio statics will poison every one of those statics permanently the first time any one of them panics while holding the lock, several `std::thread::spawn` background threads (ffmpeg stderr reader, `MemorySampler`) have no way to be told to stop short of `Drop`/process exit, and eight `mpsc::unbounded_channel` uses carry raw audio or transcript data with no cap, so a stalled consumer (a slow encoder, a stalled UI) grows memory for as long as recording continues. None of this is covered by the in-flight `add-panic-hook-logging` change, which only records panics after the fact — it does not change locking, thread lifecycle, or channel capacity.

## What Changes

- Fix `attempt_device_reconnect` (recording_commands.rs:1742-1801) so the `RECORDING_MANAGER` guard is never held across `.await`: take the manager out of the `Mutex` under the lock (mirroring the existing pattern already used in `stop_recording` at recording_commands.rs:927-930), drop the lock, run the async reconnect on the owned manager, then put it back under a fresh lock acquisition.
- Add a `lock_or_recover` helper (poison-safe lock acquisition via `unwrap_or_else(|p| p.into_inner())`, with a `log::error!` on the recovery path) and adopt it at every `.lock().unwrap()` call site in `recording_commands.rs` (~40 sites) and `recording_state.rs` (~38 sites), plus `live_diarization_reconcile.rs:115,210`. `buffer_pool.rs` and `diarization.rs` are left with a documented follow-up (diarization.rs is being restructured by change 05; buffer_pool.rs's 4 sites are in scope for a later pass, not blocking here since they hold no cross-await risk).
- Give the ffmpeg stderr reader thread (diarization.rs:2203) and `MemorySampler` (diarization.rs:2637, `Drop` at 2621) an explicit stop signal (`AtomicBool` checked in the read/poll loop, or a stop channel) in addition to the existing `Drop`-based join, so callers can request a bounded-time stop instead of relying only on process exit or struct drop.
- Convert the audio-frame-carrying `mpsc::unbounded_channel` uses to bounded channels with a documented drop policy: `pipeline.rs:1303` (raw captured `AudioChunk`s into `AudioPipeline`), `recording_manager.rs:75` (VAD-merged chunks into transcription), `recording_commands.rs:607` (embedding channel for online diarization), and `recording_saver.rs:182` (chunks into the recording accumulator). Leave `recording_commands.rs:623` (speaker turns to the frontend), `post_processor.rs:36-37`, `batch_processor.rs:29`, `device_monitor.rs:90`, and `async_logger.rs:25` unbounded — these carry low-rate control/result messages, not raw audio, so unbounded growth during a long recording is not a realistic risk.
- Add regression tests: a poisoned-mutex recovery test, a bounded-channel drop-policy test, and a reconnect-does-not-block-stop test (simulated slow reconnect, asserting `stop_recording`'s lock acquisition is not delayed by it).

## Capabilities

### New Capabilities
- `recording-concurrency-safety`: behavior guarantees for the recording engine under concurrent device/reconnect/thread/channel pressure — commands stay responsive during a slow reconnect, a panic on one audio worker does not permanently disable unrelated recording commands, and transcript/audio delivery does not grow memory unboundedly when a consumer stalls.

### Modified Capabilities
<!-- None: `audio-engine`'s existing "Device detection and reconnection" requirement already covers reconnect *attempts*; this change does not alter that behavior, it only removes a lock-holding bug in how the attempt is invoked, which is why the new guarantees live in the new capability rather than as a modification there. -->

## Impact

- Code: `frontend/src-tauri/src/audio/recording_commands.rs`, `recording_state.rs`, `recording_manager.rs`, `pipeline.rs`, `recording_saver.rs`, `live_diarization_reconcile.rs`, `diarization.rs` (thread lifecycle only, not the restructure change 05 owns).
- No new dependency: `parking_lot` is only a *transitive* dependency today (`Cargo.lock` resolves `parking_lot 0.12.5` for other crates; it is not in `meetily`'s own `[dependencies]` in `frontend/src-tauri/Cargo.toml`). Design.md compares adopting it directly against the `lock_or_recover` helper and explains why the helper is chosen.
- Out of scope: the diarization engine facade and its internal locking/threading (change 05 owns that restructure), `database/repositories/speaker.rs` locking (unrelated tables), the panic hook itself (change `add-panic-hook-logging`, assumed to land independently — this change does not duplicate it and does not depend on its code, only on the fact that it does not touch locks/threads/channels).
