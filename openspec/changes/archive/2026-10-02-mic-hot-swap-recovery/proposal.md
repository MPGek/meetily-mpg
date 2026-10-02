# Proposal

## Why

If the microphone disconnects mid-recording (USB unplug, Bluetooth headset powered off), the mic channel stays silent for the rest of the meeting. Nothing tells the user. `AudioDeviceMonitor` is started for every session (`audio/recording_manager.rs:44,144-151`) and detects the disconnect. But nothing reads its events: the only reader is the `poll_audio_device_events` command (`audio/recording_commands.rs:344`), and no frontend code calls it (`grep poll_audio_device_events frontend/src` is empty). The stream error callback emits `recording-error` (`audio/pipeline.rs:620-647`, `audio/recording/lifecycle.rs:197-199`), and the frontend does not listen for that either. Upstream v0.4.1 (#748, merge `f82a9b2`) fixed this with a backend recovery path. This change ports the cross-platform core of that fix onto the fork's split recording modules. Windows is the primary target.

## What Changes

- A backend task per recording session consumes device-monitor events. Frontend polling is not needed. When the active microphone disconnects, the task switches the mic to the current system default input. The system-audio stream keeps running throughout.
- The switch never holds the global recording-manager lock across stream teardown, stream creation or any `.await`. A switch that belongs to a stopped or replaced session is discarded, so Stop is never blocked and Stop→Start never receives a stale switch.
- Each session gets a bounded retry budget (3 failed attempts). After a successful switch the monitor tracks the new device. A device that comes back after the fallback is ignored: the fallback stays for the rest of the meeting.
- New backend events `mic-device-switched`, `mic-swap-failed` and `mic-recovery-exhausted`, shown as in-app toasts through the typed IPC layer.
- At start, a preferred or explicit mic that does not enumerate resolves to the system default by its enumerated name, and the user is notified. Today Windows opens the default silently while the monitor watches the missing name (`audio/devices/platform/windows.rs:181-194`).
- Continuity across the swap gap. Speech in progress on the dead mic is closed at the swap. The saved stereo file keeps its mic track aligned in time with system audio and the transcript, because silence fills any part of the gap that system audio did not already cover. The replacement device may run at a different sample rate (16 kHz HFP, 44.1 kHz, 48 kHz); it is resampled to the 48 kHz pipeline rate like any start-time device.
- **BREAKING (internal IPC)**: remove the dead commands `poll_audio_device_events`, `get_reconnection_status` and `attempt_device_reconnect`, plus their supporting reconnect state. No frontend code calls them. The new task takes over the event stream, which would leave `poll_audio_device_events` permanently empty. `attempt_device_reconnect` restarts both streams, which conflicts with a mic-only swap.

Out of scope:
- macOS Core Audio device-change listener and cold-start "audio wake" (upstream-only).
- Recovering a lost system-audio device.
- Switching back when the original device reconnects.
- Device-picker UI changes.
- Upstream's "no mic at start → system-audio-only recording".

## Capabilities

### New Capabilities
- `mic-disconnect-recovery`: what happens when the recording microphone becomes unavailable:
  - start-time resolution to a present device
  - backend-driven detection and fallback to the system default
  - session-scoped guards against stale or concurrent switches
  - the retry budget
  - user notifications
  - continuity of the transcript, the live diarization channel and the saved mic track across the switch

### Modified Capabilities
- `audio-engine`: the "Device detection and reconnection" requirement changes. It currently promises automatic reconnection of the same device and a notification within 2 seconds. It becomes "detect a mic disconnect and fall back to the default input with a notification", with a realistic detection bound.
- `recording-concurrency-safety`: the "Recording commands stay responsive during a device reconnect" requirement is about the `attempt_device_reconnect` command, which this change removes. It is restated for the mid-recording mic switch: Stop and the other commands are never blocked by a switch in progress.

## Impact

- Backend (`frontend/src-tauri/src/audio/`):
  - new `recording/device_recovery.rs`
  - `recording/lifecycle.rs` (spawn the processor on both start paths, shared mic resolution)
  - `recording/devices.rs` (resolution checks enumeration)
  - `recording_manager.rs` (take receiver, take/install mic stream, remove dead reconnect methods)
  - `stream.rs` (take/set the mic stream)
  - `device_monitor.rs` (retarget mailbox, re-fire while the mic is missing)
  - `pipeline.rs` (mic discontinuity marker: VAD close and gap fill)
  - `recording_state.rs` (marker send, remove reconnect fields)
  - `recording_commands.rs` (remove the 3 dead commands and their types)
- Also `frontend/src-tauri/src/lib.rs` (remove the 3 command registrations).
- Frontend:
  - `src/lib/ipc/recording.ts` (3 typed listeners)
  - `src/services/recordingService.ts` (1-to-1 wrappers)
  - `src/contexts/RecordingStateContext.tsx` (toasts)
  - a small pure toast-copy helper with bun tests
- No new dependencies, DB or settings changes. The saved-file format is unchanged.
- Related in-flight work:
  - `port-upstream-041-quick-fixes` also edits `audio/pipeline.rs` (`AudioPipeline::new` returns `Result`) and `audio/recording_manager.rs::start_recording`. The edits sit in different functions; whichever lands second rebases.
  - `harden-model-downloads` only adds a command registration in `lib.rs`.
  - `summary-run-integrity` touches none of these files.
