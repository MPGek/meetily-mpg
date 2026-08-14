# meeting-audio-player Specification

## Purpose
Playback of saved meeting recordings with streaming audio, per-utterance seek, and live transcript highlight.

## Requirements
### Requirement: Meeting audio file resolution
The system SHALL resolve the audio file path for a saved meeting via a `get_meeting_audio_path` Tauri command that looks up the meeting's `folder_path` in the database and runs canonical audio-file discovery (the same candidate list and folder scan used by retranscription and diarization).

#### Scenario: Meeting with a recording
- **WHEN** a meeting has a `folder_path` containing a known audio file (e.g. `audio.mp4`, `audio.m4a`, or any audio-extension file in the folder)
- **THEN** the command SHALL return the absolute path of that audio file

#### Scenario: Meeting without audio
- **WHEN** a meeting has no `folder_path` or the folder contains no audio file
- **THEN** the command SHALL return `null`

### Requirement: Audio player on meeting notes page
The meeting details page SHALL display an audio player bar in the left transcript panel, positioned below the top button group, whenever the meeting has a resolvable audio file.

#### Scenario: Player visible for meetings with audio
- **WHEN** a meeting with a resolvable audio file is opened on the meeting details page
- **THEN** the player bar SHALL be visible below the top button group and SHALL load the meeting's audio

#### Scenario: Player hidden for meetings without audio
- **WHEN** a meeting has no resolvable audio file
- **THEN** no player bar SHALL be displayed

#### Scenario: Player controls
- **WHEN** the player is displayed and audio is loaded
- **THEN** it SHALL show play/pause, a seek bar, and current-time/duration readouts, and the user SHALL be able to play, pause, and seek

#### Scenario: Player load failure
- **WHEN** the audio file cannot be decoded for playback
- **THEN** the player SHALL attempt an automatic FFmpeg transcode to WAV and retry playback; if that also fails, the player SHALL display an error state without crashing the page

#### Scenario: Player cleanup on navigation
- **WHEN** the user navigates away from the meeting details page
- **THEN** playback SHALL stop and audio resources SHALL be released

### Requirement: Playback from transcript utterance start time
The system SHALL seek the meeting audio player to a transcript utterance's `audio_start_time` and resume playback when the user activates the play button on that utterance's block.

#### Scenario: Play from utterance while player is idle
- **WHEN** the user clicks the play button on a transcript block with an `audio_start_time` and the player is not playing
- **THEN** the player SHALL seek to that block's start time and begin playback

#### Scenario: Play from utterance while player is playing
- **WHEN** the user clicks the play button on a transcript block while the player is playing at a different position
- **THEN** the player SHALL seek to that block's start time and continue playing

#### Scenario: Utterance without audio time
- **WHEN** a transcript block has no `audio_start_time`
- **THEN** no play button SHALL be shown for that block

#### Scenario: Audio loaded lazily
- **WHEN** the user activates a play button before the player has finished loading audio
- **THEN** the system SHALL load the audio, then seek to the utterance start time and begin playback

### Requirement: Streaming playback engine
The meeting audio player SHALL play recordings through an HTML `<audio>` element that streams the file via the Tauri asset protocol (`convertFileSrc`), and SHALL NOT transfer the full file bytes over IPC nor decode them into an in-memory PCM buffer.

#### Scenario: Meeting with a long recording opens quickly
- **WHEN** the meeting notes page opens for a meeting with a recording longer than 30 minutes
- **THEN** the player SHALL become ready without transferring the full file over IPC, and seeking SHALL apply via the media element's native position handling

#### Scenario: Memory stays bounded
- **WHEN** a recording is played
- **THEN** the full decoded audio SHALL NOT be materialized in memory (streaming playback only)

### Requirement: Asset protocol access for meeting audio
The `get_meeting_audio_path` command SHALL register the resolved audio file path in the asset protocol runtime scope before returning it, and the application CSP SHALL allow media loading from the asset protocol origins.

#### Scenario: Recording outside the configured asset scope plays
- **WHEN** a meeting's audio file lives in the user-configured recordings folder (outside `$APPDATA`)
- **THEN** the command SHALL add the path to the asset protocol scope, and the webview SHALL be able to load it as the media element source

#### Scenario: CSP permits media from asset protocol
- **WHEN** the frontend sets an `asset:` / `asset.localhost` URL as the audio source
- **THEN** the CSP SHALL NOT block the media load

### Requirement: Pause preserves playback position
Pausing playback SHALL keep the current position displayed in the player, and resuming SHALL continue from that position.

#### Scenario: Pause keeps the clock
- **WHEN** the user pauses playback at position P
- **THEN** the player SHALL display P (not 0:00)

#### Scenario: Resume continues from P
- **WHEN** the user resumes after pausing at position P
- **THEN** playback SHALL continue from P without jumping

### Requirement: Play-from-block works in every playback state
Activating a transcript block's play button SHALL seek the player to the block's `audio_start_time` and start or continue playback regardless of the player's current state.

#### Scenario: Play from block while paused
- **WHEN** the player is paused and the user clicks a transcript block's play button
- **THEN** playback SHALL start at that block's start time

#### Scenario: Play from block while playing
- **WHEN** the player is playing and the user clicks a different transcript block's play button
- **THEN** playback SHALL seek to that block's start time and continue playing without stopping

### Requirement: Undecodable format fallback
When the media element cannot decode the audio file, the player SHALL transcode the file to WAV via the existing `prepare_audio_for_playback` command and retry playback with the transcoded file.

#### Scenario: Imported format the element rejects
- **WHEN** the media element fires an error for the meeting's audio file
- **THEN** the player SHALL invoke `prepare_audio_for_playback`, swap the source to the returned WAV, and retry playback

#### Scenario: Transcode also fails
- **WHEN** transcoding fails or the transcoded file also cannot be played
- **THEN** the player SHALL display its error state without crashing the page

### Requirement: Live highlight of the playing block
While playing, the transcript block whose recording-relative time range contains the player position SHALL be highlighted, and the highlight SHALL move to the next block as playback crosses block boundaries, for both microphone-originated and system-originated blocks.

#### Scenario: Highlight tracks playback position
- **WHEN** playback is at a position inside transcript block N's time range
- **THEN** block N SHALL be highlighted

#### Scenario: Highlight moves at block boundaries
- **WHEN** playback position crosses from block N's time range into block N+1's time range
- **THEN** the highlight SHALL move from block N to block N+1

#### Scenario: Paused position keeps highlight
- **WHEN** playback is paused
- **THEN** the block containing the paused position SHALL remain highlighted

### Requirement: Auto-scroll to the playing block
The transcript list SHALL scroll the highlighted block into view when the highlight moves due to playback.

#### Scenario: List follows playback
- **WHEN** the highlighted block changes while playing and the new block is not fully visible
- **THEN** the transcript list SHALL scroll so the block is visible

#### Scenario: No scrolling while paused
- **WHEN** the player is paused
- **THEN** the transcript list SHALL NOT auto-scroll

### Requirement: End of playback clears highlight
When playback ends naturally, the highlight SHALL be cleared.

#### Scenario: Playback reaches the end
- **WHEN** playback finishes the recording
- **THEN** no transcript block SHALL remain highlighted, and the player time SHALL reset

### Requirement: System default output device
The player SHALL route audio through the system default output endpoint and SHALL NOT select or pin any specific output device.

#### Scenario: Playback on any default device
- **WHEN** playback starts
- **THEN** audio SHALL go to the operating system's default output device, with no device-specific configuration in the player
