## Context

Two backend commands start a recording, and they resolve audio devices differently:

- `start_recording_with_meeting_name` (tray icon, sidebar quick-start) loads recording preferences and resolves devices via a full chain — explicit preference → `default_input_device()` / `default_output_device()` → `None` (mic required, system optional).
- `start_recording_with_devices_and_meeting` (main Record button) takes `mic_device_name` / `system_device_name` strings and resolves each as `parse_audio_device(name)` **or `None`** — with no preference/default fallback. A `None` system device causes `AudioStreamManager::start_streams` to log "No system device specified, skipping system audio stream", producing a microphone-only recording.

The frontend adds a second gap: `RecordingSettings` (settings page) persists device choices to disk via `savePreferences` but only updates its own local state, not the shared `selectedDevices` context that `useRecordingStart` reads. So a user who picks a system device on the settings page and immediately clicks Record still sends `null` (the context is loaded only once, at app startup).

Confirmed at runtime: startup log shows `system=None` preference; recording start log shows "No system device specified, skipping system audio stream" and "1 active streams" (mic only). The quick-start path already behaves correctly; only the device-specific path is broken.

## Goals / Non-Goals

**Goals:**
- Make the main Record button capture system audio by default when no explicit system device is chosen, matching the quick-start path's behavior.
- Give microphone resolution the same fallback chain for symmetry and robustness.
- Make settings-page device changes take effect on the main page without an app restart.

**Non-Goals:**
- Changing the tray/sidebar quick-start path (already correct).
- Changing the audio pipeline, mixing, transcription, or diarization.
- Changing the saved preferences format or the Tauri command surface (names/arguments stay the same).
- Reworking the two-device-selector UI into a single source of truth (left as a future cleanup; the propagation fix is the minimal step).

## Decisions

### Decision 1: Mirror the existing fallback chain in `start_recording_with_devices_and_meeting`

**Chosen**: In `recording_commands.rs`, extract the device-resolution logic from `start_recording_with_meeting_name` into a shared helper (or inline a copy) and apply it to `start_recording_with_devices_and_meeting`. Resolution order for the system device becomes: `system_device_name` (if `Some`, `parse_audio_device`) → saved `preferred_system_device` (if `Some`) → `default_output_device()` → `None` (still optional). Same for microphone: `mic_device_name` → `preferred_mic_device` → `default_input_device()` → error if still `None` (mic remains required).

**Rationale**: This is the smallest correct fix and reuses logic that already exists and is proven in the quick-start path. It fixes the confirmed symptom (silent system audio) regardless of frontend state, because `None` from the frontend now resolves to the default output device instead of disabling capture.

**Alternative considered**: (a) Fix only the frontend (propagate context / re-read prefs at recording time) — rejected as insufficient: a fresh install with no saved preference would still send `null` and skip system audio; the backend should not silently drop a whole channel because a string is absent. (b) Force system audio to always be captured — rejected: keeps the documented "system audio is optional" behavior for platforms without an output device.

### Decision 2: Frontend — `RecordingSettings` updates the shared context

**Chosen**: In `RecordingSettings.handleDeviceChange`, in addition to `setPreferences` + `savePreferences`, call `useConfig().setSelectedDevices(devices)` so the main page's `selectedDevices` context is refreshed immediately.

**Rationale**: Closes the split-brain where a persisted choice never reaches the live recording flow. Minimal, one added call, no new state. The `ConfigContext` already exposes `setSelectedDevices` and the `RecordingSettings` component is rendered within the provider tree.

**Alternative considered**: Re-read preferences in `useRecordingStart` on every record click — rejected: adds an async round-trip per click and duplicates state; propagating the context at the source of the change is simpler and keeps a single direction of data flow.

### Decision 3: Keep mic required, system optional

**Chosen**: Microphone still errors out if no device resolves (unchanged requirement); system audio remains optional (falls back to default when available, skips only when the platform reports no default output).

**Rationale**: Preserves existing guarantees — recording must have a mic, system capture is best-effort — while removing the silent-drop regression for the common Windows case where a default output device exists.

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| **Default output device differs from user intent** (stale/absent preference) | Matches the quick-start path's existing, accepted behavior; the UI still shows the resolved device and the user can override with an explicit selection |
| **`parse_audio_device` still fails for a specific name** (e.g., device renamed) | Existing fallback chain already handles this in the quick-start path; the same warn + fallback-to-default is reused, so a bad name degrades to default instead of a hard error |
| **Settings-page propagation introduces a render cycle or stale closure** | `setSelectedDevices` is a plain context setter; no new effects or subscriptions are added |
| **Behavioral difference between the two commands persists** (quick-start vs main) | The two paths now share the same resolution helper, reducing future drift; full consolidation is explicitly out of scope |

## Migration Plan

1. Land the backend resolution change in `recording_commands.rs` and verify: recording from the main button with no system device selected now logs a system stream ("Creating system audio stream") and the recording has a populated right channel.
2. Land the frontend context propagation in `RecordingSettings.tsx` and verify: selecting a device on the settings page is reflected in the main page's selector and used by the next recording without restart.
3. No data migration: preferences format and DB schema are unchanged.

## Open Questions

1. **Helper extraction vs. inline copy** — extracting a shared `resolve_devices(...)` helper touches `start_recording_with_meeting_name` too; an inline copy keeps that command untouched. Decide during implementation (leaning toward extraction to prevent drift, but keeping the quick-start path's observable behavior identical).
2. **Should mic fallback be added at all?** — the confirmed bug is system-audio-only; mic fallback is included for symmetry but is technically scope-expanding. Confirm it's desired before coding.
