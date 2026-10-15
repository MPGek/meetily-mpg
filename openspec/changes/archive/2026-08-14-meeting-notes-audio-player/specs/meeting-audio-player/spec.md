## ADDED Requirements

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
