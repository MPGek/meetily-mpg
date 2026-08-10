## Context

The app supports two local transcription providers: Whisper (`localWhisper`) and Parakeet (`parakeet`). The user selects their provider in settings, and the backend correctly dispatches validation to the right engine (`transcription/engine.rs:validate_transcription_model_ready`). However, the frontend gates recording start with a hardcoded Parakeet check in `useRecordingStart.ts`, and the tray menu does the same in `tray.rs:check_can_record`. This creates a mismatch where the frontend blocks recording even when the user's selected provider (Whisper) is fully ready.

Key files:
- `frontend/src/hooks/useRecordingStart.ts` — three recording-start paths, all with hardcoded Parakeet checks
- `frontend/src-tauri/src/tray.rs` — tray menu `check_can_record` only checks Parakeet
- `frontend/src-tauri/src/audio/transcription/engine.rs` — backend validation (correct, provider-aware)
- `frontend/src-tauri/src/whisper_engine/commands.rs` — Whisper model validation
- `frontend/src-tauri/src/parakeet_engine/commands.rs` — Parakeet model validation

## Goals / Non-Goals

**Goals:**
- Frontend recording-start gate checks the correct provider based on user's saved transcript config
- Tray menu recording gate checks the correct provider based on user's saved transcript config
- Single new backend command that returns readiness + download status for the active provider
- No duplication of provider-specific logic in the frontend

**Non-Goals:**
- Changing the backend validation logic (it's already correct)
- Adding support for remote/cloud transcription providers
- Changing how models are downloaded or managed
- Modifying the onboarding flow

## Decisions

### Decision 1: New backend command `check_active_transcription_model_ready`

**Choice**: Create a single new Tauri command that returns `{ ready: bool, provider: string, downloading: bool }` by reading the saved transcript config and delegating to the correct engine's validation.

**Alternatives considered**:
- **Reuse `validate_transcription_model_ready` directly**: It returns `Result<(), String>` — not structured enough for the frontend to distinguish "downloading" vs "not downloaded". The frontend needs the `downloading` flag to show the right toast.
- **Two separate commands (one per provider)**: Frontend would need to know which to call, duplicating provider logic. Rejected.
- **Boolean wrapper around existing validate**: Simpler but loses the `downloading` distinction.

**Rationale**: One command, structured response, frontend stays provider-agnostic.

### Decision 2: Frontend replaces all three `checkParakeetReady` calls with `check_active_transcription_model_ready`

**Choice**: Replace `checkParakeetReady` and `checkIfModelDownloading` in `useRecordingStart.ts` with a single `checkActiveModelReady` that invokes the new backend command.

**Rationale**: Eliminates the duplication (3 paths × 2 checks = 6 call sites) and removes provider-specific logic from the frontend entirely.

### Decision 3: Tray `check_can_record` uses the same backend command

**Choice**: Update `tray.rs:check_can_record` to call the same internal validation function used by the new Tauri command, instead of hardcoding `parakeet_has_available_models`.

**Rationale**: Consistency — tray and frontend use the same readiness logic. The tray already has access to the app handle, so it can call the internal function directly (no IPC needed).

## Risks / Trade-offs

- **[Risk] Config not saved yet on first run** → Mitigation: The backend command falls back to Parakeet default (matching existing behavior in `validate_transcription_model_ready`), so first-run behavior is unchanged.
- **[Risk] New command adds IPC overhead** → Mitigation: The command is called once at recording start (not in a loop). The validation it delegates to was already being called by the backend at recording start anyway. Net overhead: ~one config DB read.
- **[Risk] Tray check previously bypassed for completed onboarding** → Mitigation: `check_can_record` already returns `true` when onboarding is complete. The provider-aware check only applies during onboarding (when `can_record` matters for tray menu). We should extend it to always validate, since the backend also validates at recording start — but the tray gate should not falsely block users who have only Whisper.
