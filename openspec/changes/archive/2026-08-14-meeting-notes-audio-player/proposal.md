## Why

Users cannot verify transcription accuracy or speaker assignment against the actual recording: the meeting notes page shows transcripts and speaker labels, but there is no way to play the meeting audio. To validate a segment, the user must open the audio file externally and manually locate the timestamp. A player embedded in the meeting notes page — with per-utterance play buttons — makes validation instant.

## What Changes

- Add an audio player bar to the meeting details page, placed in the left transcript panel under the existing top button group (the user's preferred location; the transcript list is between the buttons and the bottom prompt box, so a bar under the buttons keeps controls near the content).
- Add a play button to every transcript block (utterance) that has an `audio_start_time`. Clicking it seeks the player to the utterance's start time and resumes playback, so the user hears exactly what that block claims to say.
- Wire the existing but unused `useAudioPlayer` hook (`frontend/src/hooks/useAudioPlayer.ts`) and the empty `AudioPlayer.tsx` placeholder component into the meeting details page.
- Add a backend Tauri command that resolves the meeting's audio file path (reusing the existing canonical-file discovery logic in `retranscription.rs`/`diarization.rs`), so the frontend does not duplicate filename-scanning.
- Handle formats the webview cannot decode (`decodeAudioData`): if decoding fails, transcode the audio to WAV via the bundled FFmpeg and play the transcoded file.

## Capabilities

### New Capabilities

- `meeting-audio-player`: playback of meeting recordings on the meeting notes page — audio file resolution, player UI (play/pause/seek/progress), and seeking to a transcript utterance's start time with playback resume.

### Modified Capabilities

- `split-transcript-ui`: transcript blocks in the meeting details view gain a play button (when the block has recording-relative start time and the meeting has audio) that seeks the audio player to that block.

## Impact

- `frontend/src-tauri/src/api/api.rs` — new `get_meeting_audio_path` IPC command (or equivalent) resolving the audio file for a meeting.
- `frontend/src-tauri/src/audio/retranscription.rs` / `diarization.rs` — `find_audio_file` becomes shared/accessible; new audio-to-WAV transcode helper for playback fallback.
- `frontend/src-tauri/src/lib.rs` — register new commands in the invoke handler.
- `frontend/src/components/AudioPlayer.tsx` — implement the player bar (currently an empty placeholder).
- `frontend/src/components/MeetingDetails/TranscriptPanel.tsx` — mount the player under the button group; load audio path via `meetingFolderPath`.
- `frontend/src/components/VirtualizedTranscriptView.tsx` — per-utterance play button in `TranscriptSegment`, plumbing `audio_start_time` (already partially present as `timestamp`) and play handler.
- `frontend/src/hooks/useAudioPlayer.ts` — reuse as-is; possibly extend with a transcode-fallback load path.
- No DB schema changes; `audio_start_time`/`audio_end_time` already exist on transcripts and `folder_path` already reaches the page.
