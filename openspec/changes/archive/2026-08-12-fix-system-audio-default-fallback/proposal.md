## Why

Recording started from the main Record button silently captures microphone-only audio: the system audio stream is never created. Root cause (confirmed by runtime logs) is that `start_recording_with_devices_and_meeting` — the command the main UI invokes — resolves `system_device` as `None` when the frontend sends no system device name, and then skips the stream entirely. Unlike the tray/sidebar quick-start path (`start_recording_with_meeting_name`), it has no fallback to `default_output_device()`. The frontend makes this worse: the settings-page device selector persists its choice to disk but never updates the shared `selectedDevices` context that the main button reads, so `null` is sent even after the user picks a system device.

## What Changes

- **Backend**: `start_recording_with_devices_and_meeting` (and its `start_recording_with_devices` wrapper) gains the same device-resolution fallback chain already present in `start_recording_with_meeting_name`: explicit device name → saved recording preference → `default_input_device()` / `default_output_device()`. When no system device is specified, system audio is captured from the default output device (WASAPI loopback on Windows) instead of being skipped.
- **Backend**: microphone resolution in the same command mirrors the system-audio fallback (currently a `None` mic also silently disables capture; the preference/default fallback is added for symmetry and robustness).
- **Frontend**: `RecordingSettings` device changes propagate to the shared `selectedDevices` context (via `useConfig().setSelectedDevices`) so a device picked on the settings page takes effect on the main page immediately, without an app restart.
- **No behavior change** to the tray/sidebar quick-start path, the audio pipeline, mixing, transcription, or saved preferences format.

## Capabilities

### New Capabilities

<!-- None: this is a bug fix to existing device-resolution behavior. -->

### Modified Capabilities

- `audio-engine`: recording with explicit/partial device selection now resolves unspecified devices through a preference → system-default fallback chain instead of disabling capture; the settings-page device selection propagates to the live recording flow.

## Impact

- **Affected code**: `frontend/src-tauri/src/audio/recording_commands.rs` (device resolution in `start_recording_with_devices_and_meeting`), `frontend/src/components/RecordingSettings.tsx` (context propagation on device change)
- **Affected specs**: `openspec/specs/audio-engine/spec.md` (recording device fallback + preferences persistence requirements)
- **No API/schema/dependency changes**: existing Tauri commands and preference storage are unchanged; frontend only adds a context setter call
- **Risks**: default-output resolution may pick a different device than the user intended if they had a stale preference and no explicit selection — acceptable, since it matches the quick-start path's existing behavior and guarantees system audio is captured rather than silently dropped
