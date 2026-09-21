# Design

## Context

See `proposal.md` for motivation. Current state:

- `RECORDING_MANAGER: Mutex<Option<RecordingManager>>` (a `std::sync::Mutex`, `frontend/src-tauri/src/audio/recording_commands.rs`) is the single global handle to the active recording. `RecordingManager` is not `Clone` and holds `!Send` `cpal::Stream`s indirectly via its `stream_manager` field, so it cannot be cloned or moved to another thread while active; every command that needs it locks this one `Mutex`.
- `attempt_device_reconnect` (recording_commands.rs:1742-1801) is the one place that violates the "never hold a std `Mutex` guard across `.await`" rule: it spawns a blocking task, calls `Handle::current().block_on(async { ... })` inside it, and the `async` block locks `RECORDING_MANAGER` and then `.await`s `RecordingManager::attempt_device_reconnect` (recording_manager.rs:557-616, itself awaiting `list_audio_devices()` and `stream_manager.start_streams(...)`) while the guard is alive. Every other `RECORDING_MANAGER.lock()` site in the same file (checked by reading all 16, lines 384-1765) either scopes the guard to a synchronous block (e.g. `stop_recording`'s Step 1 at recording_commands.rs:927-930: `let manager_for_cleanup = { let mut g = RECORDING_MANAGER.lock().unwrap(); g.take() };` — lock dropped, *then* `.await` on the owned value) or never awaits while holding the guard. `attempt_device_reconnect` is the only outlier, and `stop_recording` already demonstrates the exact fix pattern to reuse.
- 86 `.lock().unwrap()` sites exist on global audio statics: `recording_commands.rs` (40, includes `RECORDING_MANAGER`, `SHARED_SEGMENTS`, `SHARED_FOLDER`, `IS_RECORDING`-adjacent state), `recording_state.rs` (38, instance-level `Mutex` fields such as `audio_sender`, `microphone_device`, `system_device`, `stats`), `buffer_pool.rs` (4, a single `pool: Mutex<Vec<...>>`), plus `diarization.rs` (2, e.g. the ffmpeg stderr buffer) and `live_diarization_reconcile.rs:115,210` (the `LiveTurnRegistry.inner` mutex, read at `.publish`/`.turns`/`.is_ordered`/`.has_any_turns`/`.decidable`/`.clear`, all synchronous, no `.await` inside the lock scope). A panic while holding any one of these poisons it for the rest of the process, and every subsequent `.lock().unwrap()` on that same static then panics too — which, for `RECORDING_MANAGER`, means one panicked command makes recording permanently unusable until app restart.
- `parking_lot` is **not** a direct dependency: `meetily`'s `[dependencies]` (`frontend/src-tauri/Cargo.toml`) does not list it, and `Cargo.lock`'s `dependencies = [...]` block for the `meetily` package (verified by grep) does not include `parking_lot`. It is only present in `Cargo.lock` as a *transitive* dependency of other crates (`parking_lot 0.12.5`, used by e.g. `dashmap`/`tokio`'s optional features). It would need to be added as a new direct dependency to be used here.
- `diarization.rs:2203` spawns `std::thread::spawn(move || { ...; let _ = stderr.read_to_end(&mut buf); ... })` to drain the ffmpeg stderr pipe; it has no stop signal and only ends when the pipe closes (ffmpeg process exits) or the process exits. `MemorySampler` (diarization.rs:2611-2637) already has a `running: Arc<AtomicBool>` polled each loop iteration and joins the thread in `Drop` (2621-2626) — but nothing lets a caller request that stop *before* the value is dropped, and there is no timeout on the `join()`.
- Unbounded `mpsc` channels carrying audio: `pipeline.rs:1303` (`audio_sender`/`audio_receiver`, raw mixed `AudioChunk`s from `RecordingState::send_audio_chunk`, a **synchronous, non-async** function called from the pipeline's chunk-processing path — recording_state.rs:276-291), `recording_manager.rs:75` (`transcription_sender`/`transcription_receiver`, consumed by `transcription::start_transcription_task`), `recording_commands.rs:607` (`embedding_sender`/`embedding_receiver`, online-diarization input), and `recording_saver.rs:182` (`sender`/`receiver` into the recording accumulator). All four are fed by synchronous `.send()` calls inside `pipeline.rs`'s `flush_pending_segments` (pipeline.rs:908-919) or `RecordingState::send_audio_chunk` (recording_state.rs:276-291) — none of the producing call sites are `async fn`, so none of them can call an async, backpressure-applying `Sender::send(...).await`.
- Unbounded `mpsc` channels carrying control/result data: `recording_commands.rs:623` (`turn_sender`/`turn_receiver`, one `SpeakerTurn` per detected turn, forwarded to a Tauri `emit`), `post_processor.rs:36-37` (one LLM post-process request/response per call, not per audio frame), `batch_processor.rs:29` (generic `BatchProcessor<T>`; its only real instantiation, `AudioMetricsBatcher` in `pipeline.rs`, batches periodic audio-level metrics, not raw samples), `device_monitor.rs:90` (one `DeviceEvent` per disconnect/reconnect, inherently rare), `async_logger.rs:25` (one `LogMessage` per `log::` call routed through it — bursty but each entry is small and the existing design already batches/flushes; dropping a bounded log channel under load risks losing the very panic-adjacent log lines this system needs).

## Goals / Non-Goals

**Goals:**

- Remove the one confirmed std-`Mutex`-guard-held-across-`.await` bug (`attempt_device_reconnect`) without changing its external behavior (still returns `Result<bool, String>`, same error messages).
- Make every `.lock().unwrap()` in the three in-scope files recover instead of permanently poisoning, with a visible log line when recovery happens (so poisoning is diagnosable, not silent).
- Give the two identified untracked background threads an explicit, bounded-time stop path, without moving their ownership (that is change 05's job for `diarization.rs`'s broader restructure).
- Cap the four audio-data-carrying channels identified above with a capacity and a single documented policy (drop-and-log, chosen over backpressure — see D5) so a stalled consumer cannot grow the process's memory for the life of a recording.
- Add tests that fail before the fix and pass after, for the three most concrete claims: lock-not-held-across-slow-reconnect, poison recovery, and bounded-channel drop-under-backpressure.

**Non-Goals:**

- Redesigning `diarization.rs`'s ownership/module structure (change 05) or `recording_commands.rs`'s command/orchestration split (change 07) — this change only makes the two named threads stoppable *in place*.
- Adopting `parking_lot` project-wide, or converting every `Mutex` type in the codebase — scoped to the sites named in the proposal.
- Changing `buffer_pool.rs` or `diarization.rs`'s existing lock sites (documented as follow-up, see Migration Plan) — they carry no cross-`.await` risk today, so hardening them is lower urgency than the in-scope files.
- Building the panic hook / crash log (that is `add-panic-hook-logging`); this change assumes that hook lands independently and does not touch `frontend/src-tauri/src/lib.rs`'s `run()` or add a `panic_log` module.
- Changing the wire format or content of `SpeakerTurn`, `AudioChunk`, or any Tauri event payload.

## Decisions

### D1: Fix `attempt_device_reconnect` by taking the manager out of the lock, mirroring `stop_recording`

Change the body from "lock, then `.await` while locked" to "lock, `take()` the manager out, drop the lock, `.await` on the owned manager, lock again, put it back":

```rust
let mut manager = {
    let mut guard = RECORDING_MANAGER.lock_or_recover();
    guard.take().ok_or_else(|| "Recording not active".to_string())?
};
let result = manager.attempt_device_reconnect(&device_name, monitor_type).await;
{
    let mut guard = RECORDING_MANAGER.lock_or_recover();
    *guard = Some(manager);
}
```

This also removes the `spawn_blocking` + `Handle::current().block_on` wrapper entirely: `attempt_device_reconnect` is already `async fn`, so it can `.await` directly — the `block_on` was only needed because the previous code tried to run an async call while a lock was held across a boundary that could not otherwise await, which this fix makes unnecessary.

- Why this shape: it exactly matches the pattern already in production at `stop_recording`'s Step 1 (recording_commands.rs:927-930), so it is not a novel pattern reviewers or future maintainers need to learn.
- Consequence to call out: while a reconnect is running, `RECORDING_MANAGER` is `None`, so a concurrent `stop_recording`, `poll_audio_device_events`, or `get_reconnection_status` sees "recording not active" / no manager for that window, instead of blocking. This is a behavior change (see spec: "stays responsive" instead of "blocks"), and it matches how the code already treats a similar window in `stop_recording` itself (which also `take()`s the manager for the duration of its own async cleanup, recording_commands.rs:927-930, meaning a command arriving mid-stop already sees `None` today). Calling `stop_recording` during an in-progress reconnect will now return "Recording not active" rather than hang — a caller-visible improvement, not a regression, and it is what the new spec's "Stop completes while a reconnect is in progress" scenario requires.
- Alternative considered: wrap the manager in `tokio::sync::Mutex` instead of `std::sync::Mutex`. Rejected: it would require `.await` at all 16 call sites (including several that are not `async fn` today) and does not change the actual bug, which is holding *any* lock across a slow operation while other callers need the resource — `take()`-and-restore solves that directly with a one-function change.
- Alternative considered: an actor (a background task owning `RecordingManager`, commands send it messages over a channel). Rejected as out of scope: it is a bigger structural change better suited to change 07 (`split-recording-commands`), which already owns moving orchestration out of `recording_commands.rs`.

### D2: Poison-safety via a `lock_or_recover` extension trait, not `parking_lot`

Add, in a small new module (e.g. `frontend/src-tauri/src/audio/sync_ext.rs`):

```rust
pub trait LockRecover<T> {
    fn lock_or_recover(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockRecover<T> for std::sync::Mutex<T> {
    fn lock_or_recover(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            log::error!("Recovered a poisoned lock (a prior holder panicked); continuing with its last state");
            poisoned.into_inner()
        })
    }
}
```

Replace `.lock().unwrap()` with `.lock_or_recover()` at every site in `recording_commands.rs`, `recording_state.rs`, and `live_diarization_reconcile.rs`.

- **Compared with switching those statics to `parking_lot::Mutex`:** `parking_lot::Mutex` never poisons (its `lock()` returns the guard directly, no `Result`), which would remove the poisoning problem just as completely and with less code per call site (`.lock()` instead of `.lock_or_recover()`). It was rejected here in favor of the helper for three reasons: (1) it is **not currently a direct dependency** (see Context) — adopting it means a new line in `Cargo.toml` plus auditing that nothing downstream relies on `std::sync::MutexGuard`'s exact type (e.g. any `Send`-bound generic code, or code passing a guard across an `.await` point, which needs `parking_lot`'s `send_guard` feature to even compile — and enabling that feature is itself easy to get wrong and silently reintroduce the D1-style bug, since a `parking_lot` guard *can* then be held across `.await` without a compile error, whereas a `std::sync::MutexGuard` used that way at least usually fails the `Send` check inside `tokio::spawn`); (2) `parking_lot` does not fix the actual D1 bug — a `parking_lot::Mutex` guard held across `.await` blocks the executor thread exactly like a `std::sync::Mutex` guard does, so switching mutex types is orthogonal to the lock-across-await problem, not a fix for it; (3) the helper is a mechanical, reviewable, one-line-per-site change with no dependency or type-signature change, so it is lower-risk to land across ~78 call sites in one change.
- Recovery is logged (`log::error!`) rather than silent, so a poisoning event is still visible in logs even though it no longer cascades into permanent unavailability — this is the "diagnosable, not silent" goal.
- `buffer_pool.rs` (4 sites) and `diarization.rs`'s 2 sites are intentionally left as `.lock().unwrap()` in this change: they hold no cross-`.await` risk today (verified: all four `buffer_pool.rs` sites are synchronous, scoped, single-statement locks), so poisoning them only matters if something else in the same lock scope can panic, which is lower-probability than the in-scope files. `diarization.rs` is flagged for its restructure (change 05) to adopt `lock_or_recover` there instead of duplicating the helper's introduction across two in-flight changes.

### D3: Explicit stop signal for the ffmpeg stderr reader thread

Change the thread to check a shared `Arc<AtomicBool>` in a loop with bounded reads instead of a single blocking `read_to_end`:

```rust
let stop = Arc::new(AtomicBool::new(false));
let stop_clone = Arc::clone(&stop);
let handle = std::thread::spawn(move || {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if stop_clone.load(Ordering::Relaxed) { break; }
        match stderr.read(&mut chunk) {
            Ok(0) => break,           // pipe closed (ffmpeg exited)
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    *stderr_buf_clone.lock_or_recover() = buf;
});
```

The owning struct (`PcmStream`) keeps `stop: Arc<AtomicBool>` and `handle: Option<JoinHandle<()>>`, and gains an explicit `fn stop(&mut self, timeout: Duration)` that sets the flag and joins with a timeout (join via a helper that polls `JoinHandle::is_finished()` in a short sleep loop, since `std::thread::JoinHandle` has no native timed join), logging if the timeout elapses without leaking or blocking the caller indefinitely.

- Why polling reads instead of one blocking `read_to_end`: a blocking read cannot be interrupted by an `AtomicBool` alone; a short bounded read (or a read with a short timeout, depending on the platform pipe type) is what makes the flag check actually take effect promptly.
- `MemorySampler` already has the `AtomicBool` + loop shape (D3 is really "give `MemorySampler` a public `stop()` that callers can invoke before drop, and add a timeout to its existing `join()` in `Drop`"): add `pub fn stop(&mut self, timeout: Duration)` that does what `Drop` does today but with the same timed-join helper, and have `Drop` call it (so both paths share one implementation and `Drop` remains a safety net, not the only path).
- Scope note: both threads keep their current owning struct location in `diarization.rs`; change 05 is expected to move them, not this change.

### D4: Bounded channels — capacity and drop policy, chosen per producer's calling context

All four in-scope producers (`pipeline.rs:1303`'s `audio_sender`, `recording_manager.rs:75`'s `transcription_sender`, `recording_commands.rs:607`'s `embedding_sender`, `recording_saver.rs:182`'s `sender`) are driven by **synchronous, non-`async fn`** call sites (`RecordingState::send_audio_chunk` at recording_state.rs:276, and `AudioPipeline::flush_pending_segments` at pipeline.rs:879-921). None of them can call `tokio::sync::mpsc::Sender::send(...).await` without making their caller `async`, which would ripple into the audio-processing call chain. The policy is therefore **bounded capacity + `try_send` + drop-and-log on `Full`**, not backpressure:

| Channel | Capacity | Rationale |
|---|---|---|
| `pipeline.rs:1303` `audio_sender` (raw mixed `AudioChunk`, ~50ms mixing window per `AudioMixerRingBuffer::new`, pipeline.rs:29-33) | 128 | ~6.4s of buffered audio at the 50ms window size before drop — enough to absorb a brief consumer stall without hiding a real, sustained backlog. |
| `recording_manager.rs:75` `transcription_sender` (VAD-merged segments, one per utterance, seconds each) | 32 | Segments are already coalesced by VAD merging (up to 25s each, `pipeline.rs:889`), so 32 pending segments is a large multi-minute backlog — well past the point where dropping and logging is more useful than silently growing. |
| `recording_commands.rs:607` `embedding_sender` (same merged segments, online-diarization path) | 32 | Same reasoning as `transcription_sender`; it receives the same `transcription_chunk` (pipeline.rs:919). |
| `recording_saver.rs:182` `sender` (mixed chunks into the on-disk accumulator) | 128 | Matches `audio_sender`'s reasoning; this is the path `recording-save-durability`'s existing checkpoint-failure requirements already assume can occasionally drop a segment, so a bounded-drop here is consistent with an existing accepted failure mode, not a new one. |

Policy: `try_send`; on `Err(TrySendError::Full(_))`, log a `warn!` including the channel name and drop the chunk (the sender already treats `Err` from `.send()` as non-fatal today — recording_state.rs:279-281 turns a closed-channel send error into an `anyhow` error that is logged and does not stop the caller's loop, and pipeline.rs:908-921 already `warn!`/`debug!`s and continues on `Err` — so drop-and-log is a small extension of an existing, already-accepted error path, not a new failure mode).

- Alternative considered: block the producer (`try_send` retried in a spin/short-sleep loop until space frees). Rejected: `send_audio_chunk` and `flush_pending_segments` are called from the same synchronous path that also drives VAD and mixing state; blocking there risks stalling *all* device audio, not just the slow consumer's, which is worse than dropping some data for the slow consumer alone.
- Alternative considered: leave these unbounded but add a periodic "queue depth" metric/log instead of a hard cap. Rejected: a metric alone does not stop the memory growth the spec requires bounding; it only makes the growth observable.
- The four control/result channels named in the proposal (`recording_commands.rs:623`, `post_processor.rs:36-37`, `batch_processor.rs:29`, `device_monitor.rs:90`, `async_logger.rs:25`) are left unbounded: each carries one message per discrete event (a detected speaker turn, one LLM request/response pair, a periodic metrics batch, a device connect/disconnect, one log line), at a rate orders of magnitude below raw audio delivery, so an unbounded backlog there would have to come from a sustained structural stall (e.g. the frontend never draining events), which is a different, already-observable failure mode (the app would appear frozen) rather than the silent memory growth this change targets.

## Risks / Trade-offs

- **[Risk] Dropping audio chunks under backpressure means audible/transcript gaps, not just a performance hint.** → Mitigation: capacities in D4 are sized to absorb multi-second to multi-minute stalls before any drop occurs, and every drop is logged with the channel name and a chunk/segment identifier, so a support investigation can see exactly when and how much was dropped; `recording-save-durability`'s existing partial-audio-warning behavior already surfaces file-level gaps to the user, and a future change can wire a drop counter into that same surface (left as an Open Question below, not solved here).
- **[Risk] `take()`-then-restore in D1 creates a window where `RECORDING_MANAGER` is `None` during a reconnect, changing observable behavior for concurrent callers.** → Mitigation: this mirrors an existing, already-shipped window (`stop_recording`'s own `take()`), so it is a known and accepted shape in this codebase, not a new kind of race; the new spec requirement makes the "stays responsive" behavior explicit and testable instead of implicit.
- **[Risk] Polling-based thread stop (D3) adds latency to shutdown (bounded by the poll/read interval) instead of an instant signal.** → Mitigation: pick a short interval (recommend 50-100ms bounded reads / poll ticks) so worst-case added shutdown latency is small and far below any timeout a caller would set; document the timeout parameter so callers choose the bound explicitly rather than the module hardcoding one value for every caller.
- **[Risk] The `lock_or_recover` helper changes behavior on poison from "panic loudly forever" to "recover and log."** → This is the explicit goal (see Requirement: poisoned lock does not permanently disable commands), but it does mean a genuinely corrupted invariant left by the panicking thread is now silently carried forward into the next lock use instead of surfacing as a hard failure. Mitigation: the log line at recovery time is `error`-level and states that a prior holder panicked, so operators/devs still see it; this change does not attempt to validate or repair the recovered state's *contents*, only to keep the lock usable.
- **[Trade-off] Scoping poison-safety to three files, not `buffer_pool.rs`/`diarization.rs`.** → Accepted: keeps this change's diff reviewable and avoids duplicating helper-introduction work that change 05 will already touch for `diarization.rs`.

## Migration Plan

- Purely internal, additive/behavioral change; no data migration, no IPC/API surface change (`attempt_device_reconnect`'s signature and `Result<bool, String>` return type are unchanged).
- Land in one change; no partial-rollout concern beyond normal code review, since every touched call site is mechanical (`.lock().unwrap()` → `.lock_or_recover()`, `unbounded_channel()` → `channel(N)` + `send` → `try_send`).
- Follow-up (not part of this change, flagged for later or for change 05/07): adopt `lock_or_recover` in `buffer_pool.rs` and (as part of change 05's restructure) in `diarization.rs`; reconsider whether `recording_commands.rs:623`'s turn channel needs a cap if a future frontend change makes the UI consumer capable of stalling for long periods.

## Open Questions

- Should a channel drop event (D4) increment a counter surfaced through the existing telemetry/session-status path (`online-diarization-telemetry`), so a user can see "N audio segments dropped" rather than only a developer reading logs? Deferred: no existing telemetry field covers this, and adding one is a larger, separate proposal.
- Should the bounded-read interval for the ffmpeg stderr reader (D3) be a fixed constant or a parameter threaded from the caller? Deferred to implementation; either is compatible with the stated requirement.
