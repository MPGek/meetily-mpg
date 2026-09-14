# Proposal: hide-alignment-ffmpeg-console

## Why

When the user presses the "Speakers" button (offline re-diarization) or stops a recording with word alignment enabled, console windows flash repeatedly on Windows. The alignment repair path spawns ffmpeg per uncached audio-span extraction without the `CREATE_NO_WINDOW` flag, which every other ffmpeg spawn in the codebase already sets.

## What Changes

- Add the Windows `CREATE_NO_WINDOW` creation flag to the ffmpeg spawn in `FileSpanSource::extract` (`frontend/src-tauri/src/audio/word_alignment/refine.rs`), matching the established pattern in `diarization.rs`, `voiceprint_clips.rs`, `encode.rs`, and other audio modules.
- No behavior, API, or configuration changes beyond the absence of visible console windows during alignment repair (offline re-diarization and stop-time finalize).

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `ctc-word-alignment`: audio-span extraction for the live-stop and offline repair paths SHALL run without visible console windows on Windows.

## Impact

- Code: one file, `frontend/src-tauri/src/audio/word_alignment/refine.rs` (single `#[cfg(target_os = "windows")]` block before `.output()`).
- Affected flows: `refine_offline_rows` (Speakers button) and stop-time repair in `recording_commands.rs`; both share `FileSpanSource`.
- No database, IPC, dependency, or frontend impact.
