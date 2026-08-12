## 1. Backend device-resolution fallback

- [x] 1.1 In `frontend/src-tauri/src/audio/recording_commands.rs`, extract (or replicate) the device-resolution chain from `start_recording_with_meeting_name` into a helper that resolves mic and system devices from `(explicit_name, preferred, default)` and apply it in `start_recording_with_devices_and_meeting` — verify: `cargo build` passes
- [x] 1.2 Ensure system-audio resolution falls back to `default_output_device()` when no explicit system device name is provided (and preference is unset), and microphone resolution falls back to `default_input_device()` (mic remains required) — verify: code inspection shows no path that returns `None` system device when a default output exists
- [x] 1.3 Keep `start_recording_with_devices` wrapper behavior unchanged (delegates to the meeting variant) — verify: no signature change to Tauri commands

## 2. Frontend device-selection propagation

- [x] 2.1 In `frontend/src/components/RecordingSettings.tsx`, use `useConfig()` and call `setSelectedDevices(devices)` inside `handleDeviceChange` (in addition to the existing `setPreferences` + `savePreferences`) — verify: TypeScript compiles, no unused-import warnings
- [x] 2.2 Confirm `RecordingSettings` is rendered inside the `ConfigProvider` tree (so `useConfig` is valid) — verify: app renders without a provider error

## 3. Verification

- [x] 3.1 Run `cargo check`/`cargo build` in `frontend/src-tauri` — verify: builds with no new errors
- [ ] 3.2 Manual smoke: start a recording from the main button with no system device selected; confirm the Rust log shows "Creating system audio stream" (not "No system device specified") and the saved stereo audio has a populated right channel (system audio)
- [ ] 3.3 Manual smoke: change the system audio device on the settings page, then start a recording from the main page without restarting; confirm the newly selected device is used
- [x] 3.4 Run `openspec validate --change fix-system-audio-default-fallback` and fix any spec/format issues
