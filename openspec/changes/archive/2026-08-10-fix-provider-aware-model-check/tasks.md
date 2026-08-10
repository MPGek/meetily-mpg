## 1. Backend: New provider-aware readiness command

- [x] 1.1 Add `check_active_transcription_model_ready` Tauri command in `audio/transcription/commands.rs` (new file or alongside existing recording_commands). It reads transcript config via `api_get_transcript_config`, dispatches to Whisper or Parakeet validation, and returns `{ ready: bool, provider: string, downloading: bool }`.
- [x] 1.2 Add download-status detection: for the active provider, check if any model has `Downloading` status in its discover_models output to populate the `downloading` field.
- [x] 1.3 Register the new command in `lib.rs` Tauri command list.

## 2. Backend: Tray provider-aware check

- [x] 2.1 Update `tray.rs:check_can_record` to read the transcript config and validate the correct provider, instead of calling only `parakeet_has_available_models`. Reuse the internal validation logic from `transcription/engine.rs`.

## 3. Frontend: Replace hardcoded Parakeet checks

- [x] 3.1 In `useRecordingStart.ts`, replace `checkParakeetReady` and `checkIfModelDownloading` with a single `checkActiveModelReady` function that invokes `check_active_transcription_model_ready` and returns `{ ready, provider, downloading }`.
- [x] 3.2 Update the manual recording-start path (line ~88) to use `checkActiveModelReady`.
- [x] 3.3 Update the auto-start path (line ~157) to use `checkActiveModelReady`.
- [x] 3.4 Update the sidebar direct-start path (line ~245) to use `checkActiveModelReady`.

## 4. Verification

- [x] 4.1 Build the backend (`cargo check` / `cargo build`) and confirm no compilation errors.
- [x] 4.2 Build the frontend (`npm run build` or equivalent) and confirm no type errors.
- [x] 4.3 Manual test: download only Whisper, select Whisper, start recording — should succeed.
- [x] 4.4 Manual test: download only Parakeet, select Parakeet, start recording — should succeed.
- [x] 4.5 Manual test: download Whisper only, have model downloading, try to record — should show "downloading" toast.
- [x] 4.6 Manual test: tray menu with only Whisper downloaded — should show "Start Recording" enabled.
