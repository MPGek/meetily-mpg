## Context

The meeting notes page (`frontend/src/app/meeting-details/`) shows transcripts with recording-relative timestamps (`audio_start_time`/`audio_end_time` on every segment) and speaker labels from diarization, but offers no way to play the recording. A user validating transcription accuracy or speaker assignment must open the audio file externally and manually seek to the timestamp.

Playback infrastructure already exists but is unwired:

- `frontend/src/hooks/useAudioPlayer.ts` — a complete, currently unused Web Audio API player (`play`/`pause`/`seek`/`isPlaying`/`currentTime`/`duration`/`error`) that reads file bytes via the existing `read_audio_file` IPC command and decodes them with `AudioContext.decodeAudioData`.
- `frontend/src/components/AudioPlayer.tsx` — an empty placeholder component.

Facts that constrain the design:

- Recordings live in the meeting folder (`folder_path` on the meeting DB row, already passed to `TranscriptPanel` as `meetingFolderPath`): `audio.mp4` for live recordings, `audio.<ext>` for imports. Canonical discovery logic exists as private `find_audio_file` in `retranscription.rs:136-164` (duplicated in `diarization.rs`).
- Tauri v2; `assetProtocol.enable = true` but its scope is `$APPDATA/**` only — recordings default to `%USERPROFILE%\Music\meetily-recordings`, so `convertFileSrc` cannot stream them without scope changes. `read_audio_file` works for any path because `fs:read-all` is granted.
- FFmpeg is already bundled (`externalBin: binaries/ffmpeg`) and used for checkpoint merging/encoding.
- Segments already carry `<div id="segment-<id>">` anchors, and `TranscriptSegmentData.timestamp` is `audio_start_time` in seconds — the seek target is already plumbed to the segment component.

## Goals / Non-Goals

**Goals:**

- Audio player on the meeting notes page, in the left transcript panel directly under the existing top button group.
- A play button on every transcript block with an `audio_start_time`; clicking it seeks the player to that block's start time and resumes playback, enabling validation of text and speaker assignment.
- Resolve the meeting's audio file path from the backend, reusing existing discovery logic.
- Graceful handling when the webview cannot decode the recording format (transcode via bundled FFmpeg to WAV).

**Non-Goals:**

- Waveform visualization, speed controls, volume UI, playlist/track switching.
- Auto-scroll / highlight of the currently playing segment; per-segment stop-at-`audio_end_time` (start-time seek only).
- Playback of the in-progress live recording session (meeting must be saved).
- Editing or trimming audio; export features.
- Changing the asset protocol scope or adding new frontend audio dependencies.

## Decisions

### D1: Player placement — left panel, under the button group

The player bar mounts inside `TranscriptPanel` between `TranscriptButtonGroup` and the transcript list. The user asked for "top panel, under the buttons" or a bottom panel; the top position keeps the player near the utterance play buttons and the transcript content, and is a localized change (no layout restructuring of `page-content.tsx`). A bottom docked bar would span the page and require restructuring `page-content.tsx`'s flex layout for no functional gain. The component stays self-contained so relocation is trivial later.

### D2: Reuse `useAudioPlayer` (Web Audio API + `read_audio_file`) as the engine

The hook already implements the exact semantics needed: `seek(time)` stops and restarts playback at `time` (resuming if it was playing), `play()` starts from `seekTimeRef`, cleanup on unmount closes the context. Alternatives considered:

- `HTMLAudioElement` + `convertFileSrc` streaming: best memory profile, but the asset protocol scope (`$APPDATA/**`) does not cover the recordings folder (default `Music/meetily-recordings`, user-configurable), so this requires runtime scope management and a configurable allowlist — complexity and security surface not justified for this iteration.
- howler.js / wavesurfer: new dependency for behavior the existing hook already provides.

The Web Audio approach decodes the full file into an in-memory `AudioBuffer`; acceptable for typical meeting sizes (see R1).

### D3: New `get_meeting_audio_path` IPC command, reusing `find_audio_file`

Add `get_meeting_audio_path(meeting_id) -> Result<Option<String>, String>` in `api/api.rs`. It looks up `folder_path` from the DB and runs the canonical audio-file discovery. To avoid the third copy of that logic, extract `find_audio_file` into a shared module (`audio/audio_file.rs`) and have `retranscription.rs` and `diarization.rs` call it. Alternatives: replicating the candidate-name scan in TypeScript (no existing directory-listing IPC, duplicates a list that already lives in Rust), or returning the path from `api_get_meeting_metadata` (changes a stable contract used elsewhere). Returns `None` when the meeting has no `folder_path` or no audio file — the player then stays hidden.

### D4: Decode failure → FFmpeg transcode to WAV, cached in temp

`decodeAudioData` handles AAC-in-MP4 on Chromium-based WebView2 in practice, but codec support is not guaranteed across every file the app accepts (imports allow `.wma`, `.webm`, `.mkv`, etc. per the candidate list). On decode failure, the hook invokes a new `prepare_audio_for_playback(file_path) -> String` command that transcodes to WAV in the temp dir (bundled FFmpeg, same invocation pattern as existing `encode.rs`/`incremental_saver.rs` code) and returns the WAV path; the hook then loads and plays that. The transcode is cached by (path, mtime) so repeated visits don't retranscode. A decode error with no successful fallback surfaces in the player's error state instead of crashing.

### D5: Utterance play button — seek + explicit play

Each `TranscriptSegment` (all three variants: legacy/mic/system) gets a small play button beside the timestamp when the segment has a start time and the meeting has audio. The click handler calls a lifted `onPlayFrom(timestamp)` that: ensures the audio is loaded (lazy-load on first interaction), calls `seek(timestamp)`, then `play()` if not already playing — the requirement is to resume playback, and the hook's `seek` alone only resumes when it was playing. The button shows a playing/paused glyph based on whether the player is at/after that segment's start; the requirement is only seek+play, so the glyph reflects the player's global `isPlaying` state. Playback state stays in the player (lifted to `TranscriptPanel`), so virtualized rows that unmount/remount (react-virtual recycling) keep correct behavior. `endTime` is not required for this feature and is not plumbed.

## Risks / Trade-offs

- [R1: Full-file decode into memory] → A 90-minute stereo recording decodes to roughly 300–600 MB PCM. Acceptable for desktop use on typical meeting lengths; if long recordings prove problematic, a follow-up can switch to streaming via asset protocol with a runtime-scoped allowlist (D2).
- [R2: `read_audio_file` returns `Vec<u8>` over JSON IPC] → For large files the payload is large and serialization adds latency. The load is one-time and lazy (first play interaction), so impact is a brief initial delay, not per-seek; a base64 payload variant is a possible optimization if it becomes noticeable.
- [R3: `decodeAudioData` rejects an accepted import format] → Mitigated by D4 transcode fallback; final failure shows the player error state.
- [R4: `find_audio_file` extraction touches two hot modules] → The extraction is a pure move with call sites updated in the same change; `retranscription.rs` and `diarization.rs` behavior is unchanged.
- [R5: Meetings without audio (text-only imports)] → `get_meeting_audio_path` returns `None`; the player bar and all play buttons are hidden, no dead UI.
- [R6: Virtualized list recycling] → Play buttons live in rows that may unmount; state is centralized in the player, and click handlers are pure callbacks, so recycling cannot desync UI.

## Migration Plan

No schema or persisted-state changes. Rollback: revert the change; the player and buttons disappear, all existing data remains valid. The `prepare_audio_for_playback` temp cache is disposable (temp dir).

## Open Questions

- Whether imported exotic formats (e.g. WMA/MKV) actually reach `decodeAudioData` failure on WebView2 in practice — D4 covers this either way.
- Whether the user wants the player to stop at `audio_end_time` when starting from an utterance (a "play this block only" mode). Current scope: play from start and keep playing; trivial to add later via the already-present `endTime` data.
