## Context

Recording folders are created in `create_meeting_folder` (`frontend/src-tauri/src/audio/audio_processing.rs:35`) as `{sanitized_meeting_name}_{UTC %Y-%m-%d_%H-%M}`. The meeting name itself is generated with a second timestamp: either the frontend `generateMeetingTitle()` (`useRecordingStart.ts:42`, `Meeting DD_MM_YY_HH_MM_SS` local) or the Rust fallback (`recording_commands.rs:226`, `Meeting %Y-%m-%d_%H-%M-%S` local, used for tray/shortcut starts). The result is a double timestamp such as `Meeting 2026-08-07_16-01-50_2026-08-07_13-01` — two dates, two formats, and the suffix in UTC.

`create_meeting_folder` has two callers: `recording_saver.rs:237` (live recordings) and `import.rs:337` (imported audio). Both go through the same naming path, so fixing it there covers all folder creation.

## Goals / Non-Goals

**Goals:**
- One timestamp per folder name, formatted `YYYY-MM-DD_HH-MM` in local time.
- Default meeting titles use the same single local format, so the folder name and the title shown in the UI match.
- Non-timestamped names (user-renamed titles, imported files) still get a disambiguating suffix.
- No two recordings ever silently share a folder (same-minute collision guard).

**Non-Goals:**
- Renaming or migrating existing folders/recordings (only new folders are affected).
- Changing the meeting title format shown in the UI beyond aligning it to the local `YYYY-MM-DD_HH-MM` pattern.
- Touching transcript/summary file naming inside folders.

## Decisions

**D1: Single source of the timestamp is the folder suffix in `create_meeting_folder`; default titles are generated to match it.**
- `create_meeting_folder` computes `Local::now().format("%Y-%m-%d_%H-%M")` (was `Utc`).
- It appends `_{timestamp}` only when the sanitized name does not already end with the pattern `_\d{4}-\d{2}-\d{2}_\d{2}-\d{2}`.
- Default titles (`generateMeetingTitle` in `useRecordingStart.ts` and the Rust fallback in `recording_commands.rs`) are changed to `Meeting %Y-%m-%d_%H-%M` local — the same format — so no suffix is appended and the name stays as-is.
- User-renamed titles (e.g. `Team sync`) don't match the pattern → `Team sync_2026-08-12_18-44` local.
- *Alternatives considered:* (a) Always append and drop the title timestamp — leaves the UI title as a bare "Meeting"; (b) always append regardless — keeps the double-date problem. Rejected both.

**D2: Minute-precision with a collision counter instead of seconds.**
- With only minute precision, two recordings started in the same minute collide. `create_meeting_folder` checks `meeting_folder.exists()` and, if so, appends `_1`, `_2`, … until free.
- *Alternatives considered:* (a) Keep seconds in the title (`%Y-%m-%d_%H-%M-%S`) — the user explicitly asked for the `_YYYY-MM-DD_HH-MM` format; (b) overwrite/reuse the folder — silently mixes transcripts of two meetings. Rejected both.

**D3: One shared timestamp helper.**
- A small private helper in `audio_processing.rs` (e.g. `fn meeting_timestamp() -> String` returning `Local::now().format("%Y-%m-%d_%H-%M")`) and a `name_has_timestamp()` check, reused by `create_meeting_folder`; the frontend mirrors the format in `generateMeetingTitle`. The Rust fallback in `recording_commands.rs` uses `Local` with the same format so tray starts produce `Meeting YYYY-MM-DD_HH-MM` and also skip the suffix.
- *Alternative considered:* centralizing name generation in Rust and removing the frontend generator — larger change touching the recording-start API; not needed since formats already converge on one string.

## Risks / Trade-offs

- [Same-minute collision behavior changed from "seconds naturally disambiguate" to "counter suffix"] → Counter guarantees uniqueness; `_1` suffix is rare and clearly readable.
- [Names that end with a timestamp pattern but are not auto-generated (e.g. user types `Retro_2026-08-12_18-44`)] → Folder keeps the name without a fresh suffix; acceptable edge case, no data loss.
- [Import path (`import.rs`) also goes through `create_meeting_folder`] → Behavior stays consistent: imported titles without the pattern get the local suffix; already-timestamped titles keep a single timestamp.
- [UTC-based folder timestamps on existing recordings won't change] → Expected; only new folders are affected.

## Migration Plan

- No data migration: existing folders and DB rows keep their paths.
- Rollback: revert the three touched locations; format reverts to the old behavior.
- Verify manually: start a recording → folder is `Meeting 2026-08-12_18-44` (local); start twice in the same minute → second folder gets `_1`; rename a meeting title and record → `Title_2026-08-12_18-44`.

## Open Questions

- None blocking. (Displayed meeting title changes from `Meeting DD_MM_YY_HH_MM_SS` to `Meeting YYYY-MM-DD_HH-MM` for new recordings — confirmed desired per request.)
