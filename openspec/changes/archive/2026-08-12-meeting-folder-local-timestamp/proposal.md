## Why

Meeting recording folders currently carry two independent timestamps (e.g. `Meeting 2026-08-07_16-01-50_2026-08-07_13-01`): one embedded in the auto-generated meeting title and one appended by the backend as a UTC suffix. The duplication is confusing, and the UTC suffix makes folder times drift from the user's local clock.

## What Changes

- Meeting recording folders use exactly **one** timestamp, formatted `YYYY-MM-DD_HH-MM` in the **local** time zone (e.g. `Meeting 2026-08-12_18-44`).
- Default meeting title generation (frontend `generateMeetingTitle` and the Rust fallback) produces `Meeting YYYY-MM-DD_HH-MM` in local time instead of the current `Meeting DD_MM_YY_HH_MM_SS` / `Meeting YYYY-MM-DD_HH-MM-SS` formats.
- `create_meeting_folder` appends its `_YYYY-MM-DD_HH-MM` suffix only when the meeting name does **not** already end with a local `YYYY-MM-DD_HH-MM` timestamp (e.g. user-renamed titles like `Team sync` still get the suffix → `Team sync_2026-08-12_18-44`).
- The appended suffix is computed with `chrono::Local` instead of `Utc`.
- If a folder with the resulting name already exists (two recordings started in the same minute), a numeric counter is appended (e.g. `Meeting 2026-08-12_18-44_1`) to prevent recordings from silently sharing a folder.

## Capabilities

### New Capabilities
- `meeting-folder-naming`: Defines how meeting recording folders are named — a single local-time `YYYY-MM-DD_HH-MM` timestamp, with a fallback suffix and collision counter for non-timestamped or duplicate names.

### Modified Capabilities

(none — no existing spec requirement changes; `audio-engine`'s requirements are unaffected)

## Impact

- `frontend/src-tauri/src/audio/audio_processing.rs` — `create_meeting_folder`: local time, conditional suffix, collision counter.
- `frontend/src-tauri/src/audio/recording_commands.rs` — Rust fallback meeting name → local `Meeting %Y-%m-%d_%H-%M`.
- `frontend/src/hooks/useRecordingStart.ts` — `generateMeetingTitle()` → local `Meeting YYYY-MM-DD_HH-MM`.
- `frontend/src/contexts/TranscriptContext.tsx` — IndexedDB fallback title kept as a fallback; now aligned to the same local format.
- No DB schema, API surface, or dependencies change. Existing recordings are untouched; only new folder names are affected.
