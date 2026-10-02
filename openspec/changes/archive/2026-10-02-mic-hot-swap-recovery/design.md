# Design

## Context

See proposal.md (Why) for the motivating bug. What exists at HEAD (`f9919e4`, branch `feat/merge_0.4.1`):

- **One `RecordingManager` per session.** Both start paths call `RecordingManager::new()` (`audio/recording/lifecycle.rs:64,384`), which builds a fresh `Arc<RecordingState>` and a fresh `AudioDeviceMonitor` (`audio/recording_manager.rs:40-55`). The manager goes into `RECORDING_MANAGER: Mutex<Option<RecordingManager>>` (`audio/recording_commands.rs:44`; stored at `lifecycle.rs:208-211,435-438`). Then `IS_RECORDING` is set (`lifecycle.rs:225,452`).
- **Stop removes the manager first.** Its first mutation is to `take()` the manager out of the slot (`audio/recording/stop.rs:43-46`). It never puts it back. `IS_RECORDING` stays true until the end of the stop tail (`stop.rs:423`). After `attempt_device_reconnect` is removed (D11), no other code takes the manager out and restores it.
- **Device monitor.** `AudioDeviceMonitor` (`audio/device_monitor.rs`) runs `list_audio_devices()` every 2 s. The interval computed at `:255-266` is never used. A device counts as disconnected after 2 consecutive missing polls (3 if its name looks like Bluetooth). It sends `DeviceDisconnected` exactly once, when the counter equals the threshold (`:241`). Names are compared exactly (`:211`). The receiver lives in `RecordingManager.device_event_receiver`. Its only reader is `poll_device_events`, behind the uncalled `poll_audio_device_events` command.
- **Start-time mic resolution does not check presence.** `resolve_microphone_device` (`audio/recording/devices.rs:13-40`) only parses the name string (`AudioDevice::from_name`, `audio/devices/configuration.rs:63-85`). `start_recording_with_meeting_name` has an inline copy of the same logic (`lifecycle.rs:90-135`). On Windows, `get_windows_device` matches by `name == base || name.contains(base)`. If nothing matches, it silently opens the default input (`audio/devices/platform/windows.rs:116-194`). In that case the session state and the monitor track a name that is not what was opened.
- **Streams.** `AudioStream::create(device, state, DeviceType, recording_sender)` (`audio/stream.rs:42-51`) has the same 4-argument signature as upstream at `f82a9b2`. The brief's note that it differs did not hold. The fork always passes `None` for `recording_sender` (`audio/recording_manager.rs:140`, `stream.rs:422-427`). Creating a stream builds a fresh `AudioCapture` with the device's native rate and channel count, a persistent resampler to 48 kHz, a mono downmix, a high-pass filter and optional RNNoise (`audio/pipeline.rs:191-357`). It also starts the stream. Chunks go into the session's bounded audio channel through `RecordingState::send_audio_chunk`, which drops chunks while paused (`audio/recording_state.rs:278-313`). Each chunk is stamped with `state.get_recording_duration()` at processing time (`pipeline.rs:572`), which is roughly the chunk's end. Its rate is 48000 after resampling (`pipeline.rs:577-588`).
- **Pipeline.**
  - Per-channel VAD dispatch buffers, with real-time anchors that map the VAD sample counter to recording seconds (`pipeline.rs:684-711,1007-1113`).
  - A ring buffer that cuts 600 ms stereo windows whenever *either* side has a full window and zero-pads the short side (`pipeline.rs:98-154`). The system-audio side therefore keeps the clock while the mic is silent.
  - Windows are sent to the saver as interleaved L=mic / R=system (`pipeline.rs:1132-1169`).
  - Control signals are chunks with reserved ids `>= u64::MAX - 10` (flush, `pipeline.rs:960-970,1392-1454`).
  - `ContinuousVadProcessor::flush()` force-ends the speech in progress and leaves the processor usable (`audio/vad.rs:561-613`).
- **Live diarization** gets 16 kHz VAD segments with recording-relative timestamps through the embedding channel (`pipeline.rs:903-930`) and keeps per-channel (mic/system) state. Nothing in it depends on the capture device.
- **Gain and ducking.** The live mic chain has no automatic gain (`pipeline.rs:483-536`, `mic-gain-and-ducking`). The ducking mixer in `audio/ffmpeg_mixer.rs` is not on the live path: the pipeline interleaves and does not mix.
- **Frontend.** All IPC goes through `frontend/src/lib/ipc/` (`listenTyped`). `RecordingStateProvider` (`frontend/src/contexts/RecordingStateContext.tsx`) is mounted app-wide and registers its event listeners once on mount. Toasts use `sonner`. No frontend code listens for `recording-error`.

## Goals / Non-Goals

**Goals:**
- Recover the mic in the background with no IPC round-trips, and never hold `RECORDING_MANAGER` across an `.await` or a cpal teardown or creation.
- Make every guard decision (stale session, stopped session, stale event, budget) a pure, unit-testable function, separate from the async and cpal glue.
- Keep the saved file and the transcript on one timeline across the gap.

**Non-Goals:**
- No new platform listener (macOS Core Audio) and no faster detection than the existing 2 s poll. The cpal stream error callback is not used as a trigger.
- No change to system-audio recovery, the device picker, recording preferences or `metadata.json` device fields.
- No change to the pipeline's channel element type (it stays `AudioChunk`).

## Decisions

### D1: One sequential device-event processor task per session

`RecordingManager::take_device_event_receiver()` moves the receiver out before the manager is stored. After the store and `IS_RECORDING = true`, both start paths call `device_recovery::spawn_device_event_processor(app, receiver, session)`, where `session` is the manager's `Arc<RecordingState>`. The task runs `while let Some(ev) = receiver.recv().await` and handles each event to completion, including any switch attempt, before reading the next. It ends when the channel closes: the monitor loop has exited at Stop and the manager is dropped at the end of `stop_recording`.

- Per-session recovery state (budget, current target) is a plain struct local to the task. There are no global `MIC_SWAP_IN_PROGRESS` / `MIC_FALLBACK_FAILED_ATTEMPTS` statics, and no `finalize_recording_start` reset to forget on a future start path.
- Alternative (upstream): spawn a task per `DeviceDisconnected` and serialize them with a global compare-exchange flag and a global counter that the start paths reset. Rejected: it needs global state that must be reset correctly on every start path. Sequential handling gives the same "one swap at a time" property for free. Events that arrive during a swap only queue up; the monitor channel is unbounded and carries only low-rate control messages (`recording-concurrency-safety` D4 of change 04).
- Alternative: keep frontend polling. Rejected: recovery would depend on the page being mounted and on a 1-2 s polling loop. That is the current bug.

### D2: Session liveness = "the manager in the slot is this session's manager and is recording"

The guard is `session_live(slot: Option<&RecordingManager>, session) = slot.is_some_and(|m| Arc::ptr_eq(m.get_state(), session) && m.is_recording())`. It is evaluated under a short `lock_or_recover()`:
- before an attempt starts
- in swap Phase 1
- in swap Phase 3 (D3)
- before any user-facing event is emitted

The pointer comparison is sound because the task holds a clone of `session`, so the allocation cannot be reused while the task runs.

- **No `StoppingGuard` / `IS_RECORDING_STOPPING` flag.** Upstream needed it because its processor checked `IS_RECORDING`, which stays true through the stop tail. Here, Stop's very first mutation empties the slot (`stop.rs:43-46`), so "slot empty" already means "stopping". Stop→Start B puts a different `RecordingState` in the slot, so the pointer check fails. A second flag would be another source of truth with its own failure mode (stuck on panic under `panic = "abort"`).
- Alternative: a session generation counter. Rejected: there is a window between storing manager B and bumping the counter in which a stale Phase 3 would install into B.
- Constraint for future code: anything that temporarily takes `RECORDING_MANAGER` out and restores it will look "stopped" to an in-flight swap. The swap aborts silently, uncounted (D4). This is documented on `session_live`.

### D3: Three-phase swap; the lock is held only for synchronous slot operations

One attempt (`attempt_mic_fallback(app, session, lost_name)`):

1. **Settle and resolve the target, without the lock.**
   - Sleep 150 ms, then call `default_input_device()`. If it still returns `lost_name`, sleep 300 ms and query once more.
   - Still the lost device, or an error → the attempt fails (D4). We never reopen the dead device.
   - Re-check `session_live`.
2. **Phase 1, under the lock.**
   - Re-check `session_live` (abort if false).
   - `manager.take_mic_stream_for_swap()`, which is `AudioStreamManager::take_mic_stream()`. It may already be `None` after an earlier failed attempt.
   - `session.mark_mic_discontinuity()` (D7).
   - Release the lock.
3. **Teardown, without the lock.** Move the old stream into `tokio::task::spawn_blocking(move || stream.stop())` and wait at most 2 s. On timeout, log and continue; the blocking thread is leaked. A cpal teardown of a vanished device can stall, and that must stall neither the runtime nor the swap.
4. **Phase 2, without the lock.**
   - Sleep 50 ms.
   - `AudioStream::create(Arc::new(AudioDevice::new(target, Input)), session.clone(), DeviceType::Microphone, None).await`. The new stream plays immediately and feeds `session`.
   - On error: wait 500 ms and retry once (as upstream). A second error fails the attempt.
5. **Phase 3, under the lock.** `install_decision(slot_state, session)` is a pure function of `Option<&Arc<RecordingState>>` (plus is-recording) and `&Arc<RecordingState>`.
   - `Install`: `manager.install_swapped_mic(stream, device)` sets the stream, `state.set_microphone_device`, and the monitor mailbox (D5).
   - `Discard(reason)`: release the lock, then drop the stream with `stop()` on a blocking thread. Log `[HOT_SWAP] discarded: <reason>`. Emit nothing.

No `.await` happens while the guard is alive. Each lock scope is a block that ends before the next await, which matches `take_drop_await_restore_does_not_hold_the_lock_across_the_await` (`recording_commands.rs:769-816`).

What happens around Stop:
- **Stop after Phase 1.** Stop's `stop_streams` finds no mic stream. The swap task owns the old stream and tears it down. Phase 3 sees an empty slot and discards the replacement.
- **Stop during Phase 2.** The replacement stream's capture callback writes to a session whose `is_recording` is false, so it drops samples (`pipeline.rs:362-364`) until Phase 3 releases it.

### D4: Retry budget, re-fire and stale-event rules (sans-IO `MicRecovery`)

`MicRecovery { current_mic: String, failed: u32 }`, with `MAX_ATTEMPTS = 3`:

- `on_disconnect(device_name, live) -> Action`:
  - `Ignore(NotLive)` if not live.
  - `Ignore(NotActiveMic)` if `device_name != current_mic`. This covers late events for a mic already swapped away from, and system-audio events.
  - `Ignore(Exhausted)` if `failed >= 3`.
  - Otherwise `Attempt`.
- `on_attempt_result(result, live) -> Option<UserEvent>`:
  - Aborted (not live): return `None`, do not count.
  - Success: `current_mic = target`, `failed = 0`, return `MicDeviceSwitched`.
  - Failure: `failed += 1`, return `MicSwapFailed { attempt, max }` if `failed < 3`, else `MicRecoveryExhausted`.

Emitting `MicRecoveryExhausted` *instead of* the third `MicSwapFailed` avoids two toasts for one outcome; upstream emitted both. The glue emits events only if `session_live` still holds at emit time. System-audio `DeviceDisconnected` and every `DeviceReconnected` / `DeviceListChanged` are only logged. The fallback is sticky, as upstream decided after the macOS cpal hang on BT re-open.

Re-fire: for **microphone** entries the monitor changes `consecutive_missing == threshold` to `consecutive_missing % threshold == 0`. While the mic is still missing it then re-sends `DeviceDisconnected` every threshold cycles (about 4 s wired, 6 s Bluetooth). That is what drives retries 2 and 3. System-audio entries keep fire-once, to avoid an endless warn-log stream for a dead output that nothing recovers. The re-fire rule is extracted into a pure `MonitoredDevice::observe(present) -> Option<Observation>` so it can be unit-tested without enumerating devices.

### D5: Monitor retarget mailbox

`AudioDeviceMonitor` gets `retarget: Arc<std::sync::Mutex<Option<String>>>` and `notify_mic_swapped(name)`. The loop `take()`s the mailbox at the start of each cycle and rebuilds the microphone entry with `MonitoredDevice::new(name, Microphone)`. That resets the counter and re-derives the Bluetooth threshold. The mutex is only touched in synchronous code (`lock_or_recover`).

- Alternative: stop and restart the monitor with the new device. Rejected:
  - `stop_monitoring` awaits the loop and would run inside Phase 3's lock scope, or need a fourth phase.
  - It adds a full re-enumeration.
  - `Notify::notify_one` stores a permit, which makes stop/start sequencing subtle.

### D6: Start-time resolution returns the enumerated device

`resolve_microphone_device(explicit, preferred)` becomes `-> Result<ResolvedMic { device, fell_back_from: Option<String> }, String>`.
- A name counts only if `find_present_input(name)` matches an enumerated input. The predicate is the same per-platform rule `get_device_and_config` uses: on Windows, exact or `contains` among WASAPI inputs; elsewhere, an exact name in `cpal::default_host().input_devices()`.
- The returned `AudioDevice` carries the **enumerated** name, so the session state and the monitor track exactly what was opened. That prevents a spurious mid-recording swap a few seconds after start.
- A name that does not match falls through to `default_input_device()`. If an explicit or preferred name was given and missed, `fell_back_from` is set.
- No default → `Err`, unchanged. Upstream's system-audio-only start is out of scope.

`start_recording_with_meeting_name`'s inline copy (`lifecycle.rs:90-135`) is replaced by a call to the helper. Both paths emit `mic-device-switched { device_name, previous_device_name, reason: "unavailable_at_start" }` when `fell_back_from` is set, after `start_recording` succeeds. Matching is a pure function `match_input_name(requested, &[String]) -> Option<String>` for tests. The cpal enumeration wrapper around it stays thin. System audio resolution is unchanged; Linux system devices are tagged Output but enumerate as inputs, which is why upstream also skipped this check for them.

### D7: Continuity in the pipeline: discontinuity marker, VAD close, timeline-preserving gap fill

**Marker.** `RecordingState::mark_mic_discontinuity()` sends `AudioChunk { chunk_id: MIC_DISCONTINUITY_CHUNK_ID (u64::MAX - 20), data: [], device_type: Microphone, .. }` with `try_send`. It skips the paused check, so a swap during pause still closes the gap. The marker uses the same channel as audio. It is sent in Phase 1 after the old stream is detached and before the new stream exists. So it is ordered after every old-mic chunk and before every new-mic chunk. `AudioPipeline::run` checks the marker id before the flush range and before normal processing.

**On the marker:**
- **(a) Close the mic VAD.** Dispatch the mic VAD buffer, call `vad_processor_mic.flush()`, then `flush_pending_segments(Microphone)`. This reuses a per-channel slice factored out of `flush_remaining_audio`, which becomes a loop over that slice. Speech in progress on the dead mic is emitted as its own segment and never merged with post-swap speech. The processor stays usable (`vad.rs:561-613`), so no reset or rebuild is needed.
- **(b) Open a gap.** If no gap is open:
  - set `mic_gap_start = last_mic_chunk_end`, where `last_mic_chunk_end` is the timestamp of the most recent mic chunk, which the pipeline already sees;
  - start `ring_buffer.mic_pad_since_gap = Some(0)`, which counts zero samples the ring buffer pads onto the mic side.

  A second marker from a later attempt does not move the gap start.

**On the first mic chunk after the marker:**
- `gap_samples = round(((chunk.timestamp − chunk_duration) − mic_gap_start) × 48000)`
- `fill = gap_samples.saturating_sub(padded)`, clamped to at most 30 s
- Push `fill` zeros into the mic side in window-sized steps. After each step run the existing window-emission loop, which is factored out of STEP 3 into `emit_ready_windows(timestamp)`. Then process the chunk normally.
- The arithmetic is a pure function `mic_gap_fill_samples(gap_start, first_chunk_start, padded, sample_rate, cap) -> usize`.

**Why fill, and why "minus padded":**
- When system audio flows during the gap, the ring buffer has already emitted windows with the mic side zero-padded. The mic track is aligned and `fill ≈ 0`.
- When system audio is silent or absent, no windows are emitted during the gap. Examples: mic-only meetings, and WASAPI loopback, which delivers nothing while the output is silent, as noted at `pipeline.rs:686-689`. Without the fill, the file would be shorter than the session by the gap. Transcript times are anchored to real time (`pipeline.rs:684-711`) and would not shrink. Every later transcript row, and the stop-time and offline speaker overlap matching that read the saved file, would be off by the gap for the rest of the meeting.

**Alternatives considered:**
- No fill: rejected for the reason above.
- Fill the full wall-clock gap unconditionally: rejected. It double-counts when system audio drove extraction, which shifts the mic later than the system channel.
- Fill in the saver: rejected. The saver only sees interleaved stereo and cannot tell which side was missing.
- Reset the mic VAD instead of flushing: rejected. It drops the in-progress speech.

The 30 s cap covers the one case where the gap is not the disconnect: the user paused during the gap. Pause discards both channels (`recording_state.rs:280-282`), but the chunk timestamps include pause time. The real gap is bounded by detection plus 3 attempts, roughly 4–20 s. Transcript timing needs no change: post-swap VAD buffers anchor to their own chunk timestamps.

### D8: Sample rate and channel changes need nothing new

Each `AudioStream::create` builds its own `AudioCapture` from the new device's `SupportedStreamConfig`. That gives it its own resampler (16 kHz HFP / 44.1 kHz → 48 kHz), its own mono downmix, and a fresh high-pass filter and RNNoise state (`pipeline.rs:191-357`). Everything downstream sees 48 kHz mono: the pipeline, VAD, ring buffer and saver. Diarization sees 16 kHz VAD segments. The `InputDeviceKind` passed to the pipeline at start is log-only (`pipeline.rs:742-749`), so it is not updated. The `[HOT_SWAP]` completion log includes the new device's native rate. Verification is manual (task 8.3) because unit tests cannot open devices.

### D9: Gain and ducking need nothing new

There is no app-level mic gain to carry over: unity gain, and OS input volume is per device and user-controlled. The new `AudioCapture` gets the same high-pass filter and RNNoise (if enabled) chain. The live path does not duck: the ffmpeg-mixer ducking is not used by `AudioPipeline`. No `mic-gain-and-ducking` delta.

### D10: Live diarization continues, no reset

The mic channel's online diarizer, cluster labels, recognized speakers and user live bindings are kept. To the diarizer, a 5–20 s gap looks like a pause. Resetting would throw away the user's live renames and pinned labels (`live-speaker-labels`) halfway through a meeting. Trade-off: the new mic changes the acoustic channel, so the same person may get a new `MIC_SPEAKER_NN` cluster. End-of-meeting refinement re-clusters all mic embeddings, and voiceprint recognition may re-bind. Manual check 8.6 observes this. No diarization delta; the requirement is stated in `mic-disconnect-recovery`.

### D11: Remove the dead reconnect surface

Remove:
- `poll_audio_device_events`, `get_reconnection_status` and `attempt_device_reconnect` (`recording_commands.rs:290-386,397-443`; `get_active_audio_output` at `:388-395` sits between them and stays), with `DeviceEventResponse`, `ReconnectionStatus` and `DisconnectedDeviceInfo`, and their `lib.rs:705-708` registrations
- `RecordingManager::{poll_device_events, attempt_device_reconnect, handle_device_disconnect, handle_device_reconnect, is_reconnecting}` (`recording_manager.rs:546-682`)
- `RecordingState`'s `is_reconnecting` and `disconnected_device` with their accessors (`recording_state.rs:108,114,236-254` and their init/cleanup lines)

`get_active_audio_output` stays: it is live and unrelated. The pattern test `take_drop_await_restore_does_not_hold_the_lock_across_the_await` stays; only its doc comment, which names `attempt_device_reconnect`, is reworded to name the swap phases.

- Alternative: keep `attempt_device_reconnect` as a manual "retry" for a future UI. Rejected: it stops and restarts **both** streams (`recording_manager.rs:589-594`), which breaks system-audio continuity and races the swap. A future manual retry should call `attempt_mic_fallback` instead.

### D12: Frontend: typed listeners, one app-wide effect, replaceable toasts

Event payloads (snake_case, as emitted):

| Event | Payload |
|---|---|
| `mic-device-switched` | `{ device_name, previous_device_name, reason: "disconnected" \| "unavailable_at_start" }` |
| `mic-swap-failed` | `{ device_name /* lost */, error, attempt, max_attempts }` |
| `mic-recovery-exhausted` | `{ device_name }` |

Frontend pieces:
- `lib/ipc/recording.ts`: `listenMicDeviceSwitched`, `listenMicSwapFailed` and `listenMicRecoveryExhausted`, built on `listenTyped`.
- `recordingService`: 1-to-1 `onMic…` wrappers, matching the file's stated policy.
- A pure `micRecoveryToast(kind, payload) -> { level: 'info'|'warning'|'error', title, description }` in `src/lib/mic-recovery-toasts.ts`.
- One mount-once effect in `RecordingStateProvider`, with the upstream `cancelled` guard for StrictMode/HMR. It calls `toast[level](title, { id: 'mic-recovery', description, duration })`. The fixed id makes the next toast replace the current one.
- `mic-swap-failed` and `mic-recovery-exhausted` toasts are also suppressed when `isRecordingRef.current` is false. The backend already gates them; the frontend gate covers an event already in flight. `mic-device-switched` is not gated, because the start-time variant arrives before `recording-started`.

These are in-app toasts only. No OS notification: the `notifications` capability is unchanged.

### D13: Log vocabulary

Log tags:
- `[DEVICE_EVENTS]` for the processor (event received, ignored with reason).
- `[HOT_SWAP]` for attempts:
  - `mic '<lost>' disconnected → target '<new>' (attempt n/3)`
  - `old stream teardown timed out`
  - `mic switched to '<new>' (<rate> Hz, <ch> ch)`
  - `attempt n/3 failed: <err>`
  - `discarded: session stopped|session changed`
  - `recovery exhausted for '<lost>'`
- The pipeline logs `[HOT_SWAP] mic gap: <secs>s, padded <n>, filled <m> samples`.

These lines are what manual verification greps for.

## Risks / Trade-offs

- [The dying stream's error callback counts toward the 10-recoverable-errors auto-stop (`recording_state.rs:323-346`)] → On Windows, cpal WASAPI reports a device invalidation once per stream, so 3 swaps cost about 3 errors. Manual check 8.1 counts `Recoverable audio error` lines per disconnect. If a device storms errors, that is a pre-existing auto-stop risk, out of scope here. Note it in the PR if seen.
- [Detection latency: 2–6 s from the 2 s poll, plus about 0.2–1 s for the swap. Speech in that window is lost, because no device exists] → This is inherent without a platform listener, and the spec allows up to 10 s. The saved file records the gap as silence, so nothing is shifted.
- [A cpal teardown or creation stalls on a vanished device] → Teardown runs on a blocking thread with a 2 s cap. Creation runs outside the lock, so only the swap task waits and Stop is unaffected. A leaked blocking thread is acceptable for a rare path.
- [A discarded replacement stream keeps the Windows mic privacy indicator on briefly after Stop] → It is released as soon as Phase 3 runs, within about 1 s.
- [The system default input may be an unwanted endpoint (for example "Stereo Mix")] → This is the same choice Windows makes and the same as start-time default resolution. The toast names the device, so the user can stop and pick another.
- [A single Bluetooth headset that is both mic and default output: its loopback (system) stream dies too and is not recovered] → Out of scope. The system-audio `DeviceDisconnected` is logged once. Open question below.
- [Pause during the gap inflates the fill] → Capped at 30 s (D7).
- [Same speaker gets a new mic cluster after the swap] → Accepted (D10). Stop-time refinement and recognition mitigate it, and the user can rename.

## Migration Plan

- No data, settings or file-format migration. The new events are additive. The removed commands have no in-repo callers (verify with `grep -rn "poll_audio_device_events\|get_reconnection_status\|attempt_device_reconnect" frontend/src` → empty).
- Land in the task-group order of tasks.md. Each group compiles and passes its tests on its own. An intermediate group may leave a not-yet-wired helper unused; group 9 requires no new clippy warnings at the end.
- Rollback: revert the commits. Nothing persists.

## Open Questions

- Should the session switch *back* to the original device when it reconnects? The case is a desktop with a single USB mic: there is no fallback, recovery is exhausted, and the user re-plugs. This change keeps upstream's sticky rule. A follow-up could act on the already-received `DeviceReconnected` without changing this design.
- Should a lost system-audio device (a Bluetooth headset as default output) be recovered the same way? Deferred to a separate change.
- Should `metadata.json`'s recorded microphone name reflect a mid-recording switch? It stays the start-time device. Analytics at stop already reads the current device (`stop.rs:276`).
