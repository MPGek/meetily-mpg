## 1. Backend: shared audio file discovery and new IPC commands

- [x] 1.1 Extract `find_audio_file` from `frontend/src-tauri/src/audio/retranscription.rs:136-164` (and its `diarization.rs` duplicate) into a shared module `frontend/src-tauri/src/audio/audio_file.rs`; update both call sites to use it (behavior unchanged)
- [x] 1.2 Add `get_meeting_audio_path(meeting_id) -> Result<Option<String>, String>` in `frontend/src-tauri/src/api/api.rs`: look up `folder_path` via the repository, run shared `find_audio_file`, return `None` when no folder or no audio file
- [x] 1.3 Add `prepare_audio_for_playback(file_path) -> Result<String, String>` in a Rust audio module: transcode the file to WAV in the temp dir using the bundled FFmpeg (same invocation pattern as `audio/encode.rs`/`incremental_saver.rs`), cached by (path, mtime) so repeat calls reuse the WAV
- [x] 1.4 Register both commands in the `invoke_handler` in `frontend/src-tauri/src/lib.rs` and add any needed module declarations
- [x] 1.5 Verify: `cargo build` in `frontend/src-tauri` compiles; `cargo test` for the audio module passes

## 2. Frontend: audio player bar on the meeting notes page

- [x] 2.1 Implement `frontend/src/components/AudioPlayer.tsx` (currently empty): compact bar with play/pause button, seek slider, current-time/duration readouts, and error state; driven by the `useAudioPlayer` hook; hidden (renders null) when `audioPath` is null
- [x] 2.2 In `frontend/src/components/MeetingDetails/TranscriptPanel.tsx`: resolve audio path on mount via `invoke('get_meeting_audio_path', { meetingId })` (meetingId prop already exists), keep `audioPath` in state, render `<AudioPlayer>` between `TranscriptButtonGroup` and the transcript list
- [x] 2.3 Expose a `playFrom(startTime)` action from the player: lazily ensure audio is loaded, then `seek(startTime)` and `play()` if not already playing; lift it up so transcript blocks can trigger it
- [x] 2.4 Extend `useAudioPlayer` (or the loader path) so a `decodeAudioData` failure invokes `prepare_audio_for_playback`, loads the returned WAV path, and retries; a second failure sets the error state (design D4)

## 3. Frontend: per-utterance play buttons

- [x] 3.1 In `frontend/src/components/VirtualizedTranscriptView.tsx` `TranscriptSegment` (all three variants: legacy, mic, system): render a small play button next to the timestamp when `timestamp` is present and a play handler is provided
- [x] 3.2 Plumb `onPlayFrom(startTime)` from `TranscriptPanel` through `VirtualizedTranscriptView` to each segment (both virtualized and simple render paths)
- [x] 3.3 Hide play buttons entirely when the meeting has no audio (no handler passed), and pass `isPlaying` down so the button shows the playing state
- [x] 3.4 Verify: TypeScript compiles (`pnpm lint` / `next build` in `frontend/`)

## 4. End-to-end verification

- [x] 4.1 Run the app (`pnpm dev` + `cargo tauri dev` in `frontend/src-tauri`), open a meeting with a recording, and confirm the player bar renders under the top buttons with play/pause/seek working
- [x] 4.2 Click a play button on a mid-meeting transcript block: confirm the player seeks to that block's `audio_start_time` and playback resumes (idle and already-playing cases)
- [x] 4.3 Confirm blocks without `audio_start_time` (or a meeting with no audio) show no play buttons and no player bar
- [x] 4.4 Confirm the transcode fallback path works on a format `decodeAudioData` rejects (e.g. an imported WMA), and the player shows an error state if transcoding also fails
- [x] 4.5 Confirm navigating away from the meeting details page stops playback and releases audio resources
