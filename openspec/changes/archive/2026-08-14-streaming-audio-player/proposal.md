## Why

The meeting-notes audio player (introduced by the in-flight change `meeting-notes-audio-player`) is built on a Web Audio engine and misbehaves in practice. Loading a 30+ minute recording transfers the entire file over IPC as a JSON byte array and decodes it into a ~700MB PCM buffer — a multi-second hang on every open. Pause/seek semantics are broken because `AudioBufferSourceNode.stop()` fires `onended`, which resets the clock to 0:00 on pause and kills newly started playback when seeking from a transcript block (a stale `onended` stops the new source). There is also no live highlight of the block being played, so validating transcription and speaker assignment against the audio is still hard.

## What Changes

- Replace the Web Audio playback engine with an HTML `<audio>` element that **streams** the recording via the Tauri asset protocol (`convertFileSrc`). No IPC byte transfer, no full decode, instant seek — fixes the slow load.
- Extend the `get_meeting_audio_path` Rust command to register the resolved path in the asset protocol runtime scope (`asset_protocol_scope().allow_file()`), and add `media-src asset:` to the CSP so the webview can stream the file.
- Fix playback semantics via native media-element behavior: pause keeps the current position, resume continues from it, and clicking a transcript block's play button starts playback when paused and seeks-and-continues when playing.
- Highlight the transcript block whose time range contains the current playback position, moving the highlight as playback crosses block boundaries (mic and system variants alike).
- Auto-scroll the transcript list to the highlighted block as playback moves.
- Clear the highlight when playback ends naturally.
- Keep playback on the system default output device — no device pinning (the Buds3 Pro setup stays device-independent).
- Keep the FFmpeg transcode fallback for formats the media element cannot decode (e.g. imported WMA): on the element's `error` event, transcode to WAV and retry.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `meeting-audio-player` (introduced by the in-flight change `meeting-notes-audio-player`; this change amends its requirements before archive — archive that change first): playback engine becomes streaming via `<audio>`/asset protocol; pause/seek/play-from-block semantics are defined; live block highlight, auto-scroll, end-of-playback clearing, and no-device-pinning are required.
- `split-transcript-ui`: transcript blocks in the meeting details view gain an active-block visual state (highlight + play/pause glyph) driven by the player position.

## Impact

- `frontend/src-tauri/src/api/api.rs` — `get_meeting_audio_path` gains an `AppHandle` parameter and registers the resolved path in the asset protocol scope.
- `frontend/src-tauri/tauri.conf.json` — CSP gains `media-src` allowing the asset protocol origins.
- `frontend/src/hooks/useAudioPlayer.ts` — rewritten around an `<audio>` element (same public API); Web Audio code removed.
- `frontend/src/components/AudioPlayer.tsx` — hidden `<audio>` element, seek-slider drag, error fallback to `prepare_audio_for_playback`.
- `frontend/src/components/MeetingDetails/TranscriptPanel.tsx` — derives the active segment id from the player position; handles auto-scroll and end-of-playback clearing.
- `frontend/src/components/VirtualizedTranscriptView.tsx` — per-row `isActive` styling (highlight + pause glyph), scroll targets.
- No DB schema changes. `read_audio_file` IPC becomes unused by the player (kept registered; removal is optional cleanup).
- Archive ordering: archive `meeting-notes-audio-player` before this change.
