## Why

The frontend recording-start gate (`useRecordingStart.ts`) hardcodes a Parakeet-only model readiness check. It calls `parakeet_has_available_models` regardless of which transcription provider the user has selected. This means users who only have Whisper downloaded cannot start recording, even though the backend correctly supports provider-aware validation. The bug manifests in three reproducible ways:

1. Download only Whisper → recording blocked ("model not ready")
2. Download Parakeet + select Whisper → recording works (frontend gate passes, backend validates correctly)
3. Delete Parakeet, only Whisper remains → recording blocked again

## What Changes

- Replace the hardcoded `checkParakeetReady` / `checkIfModelDownloading` functions in `useRecordingStart.ts` with a provider-aware check that reads the user's saved transcript config and validates the correct provider (Whisper or Parakeet).
- Add a new backend command `check_transcription_model_ready` (or reuse the existing `validate_transcription_model_ready` as a boolean query) so the frontend can ask "is the active provider's model ready?" without duplicating provider logic.
- Update the tray menu `check_can_record` (in `tray.rs`) to also be provider-aware instead of checking only Parakeet during onboarding.

## Capabilities

### New Capabilities
- `provider-aware-model-gate`: Frontend and tray recording-start gates check the correct transcription provider based on user configuration, not a hardcoded Parakeet-only check.

### Modified Capabilities
<!-- No existing spec-level requirements are changing; this is a bug fix in implementation. -->

## Impact

- **Frontend**: `useRecordingStart.ts` — all three recording-start paths (manual, auto-start, sidebar-direct) will use provider-aware checks.
- **Backend**: New or exposed Tauri command for frontend to query model readiness for the active provider. `tray.rs:check_can_record` updated.
- **APIs**: New invoke command (e.g., `check_active_transcription_model_ready`) returning `{ ready: bool, provider: string, downloading: bool }`.
- **No breaking changes**: Existing commands remain; this adds a new query endpoint.
