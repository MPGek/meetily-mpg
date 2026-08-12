## 1. Backend folder naming (Rust)

- [x] 1.1 Update `create_meeting_folder` in `frontend/src-tauri/src/audio/audio_processing.rs` to compute the timestamp with `chrono::Local` instead of `Utc`
- [x] 1.2 Add a timestamp-pattern check: append the `_YYYY-MM-DD_HH-MM` suffix only when the sanitized meeting name does not already end with `_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}`
- [x] 1.3 Add same-minute collision handling: if the resolved folder path already exists, append `_1`, `_2`, ... until a free name is found
- [x] 1.4 Update the Rust fallback meeting name in `frontend/src-tauri/src/audio/recording_commands.rs` to `Meeting %Y-%m-%d_%H-%M` using `chrono::Local`
- [x] 1.5 Add/extend Rust unit tests for timestamp detection, suffix skipping, and collision counter

## 2. Frontend title generation (TypeScript)

- [x] 2.1 Update `generateMeetingTitle` in `frontend/src/hooks/useRecordingStart.ts` to produce `Meeting YYYY-MM-DD_HH-MM` from the local date
- [x] 2.2 Align the IndexedDB fallback title in `frontend/src/contexts/TranscriptContext.tsx` to the same local `YYYY-MM-DD_HH-MM` format

## 3. Verification

- [x] 3.1 Build the Rust backend (`cargo check`/`cargo test` in `frontend/src-tauri`) and run the new tests
- [x] 3.2 Run frontend typecheck/lint (per repo conventions) to confirm no TS errors
- [ ] 3.3 Manual smoke check: start a recording and confirm the folder is `Meeting <local YYYY-MM-DD_HH-MM>` with a single timestamp
