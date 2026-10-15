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

### Requirement: Clip-level play button playback state indicator
The VoiceprintBrowser "Play clip" buttons SHALL display a visual indicator when their associated clip is actively playing, and SHALL revert to the default appearance when playback ends or a different clip is selected.

#### Scenario: Button shows playing state during active playback
- **WHEN** the user clicks a "Play clip" button and the associated clip begins playing
- **THEN** the button SHALL change its appearance to indicate active playback (e.g., icon change to Pause, color/background change)

#### Scenario: Button reverts when playback ends naturally
- **WHEN** playback of a clip finishes and the audio element reaches the end
- **THEN** the button SHALL revert to its original "Play clip" appearance

#### Scenario: Button reverts when a different clip is selected
- **WHEN** the user clicks a different "Play clip" button while another clip is playing
- **THEN** the previously-playing button SHALL revert to its original appearance, and the newly-selected button SHALL enter the playing state

#### Scenario: Only one clip button shows playing state at a time
- **WHEN** multiple "Play clip" buttons are visible in the VoiceprintBrowser
- **THEN** at most one button SHALL display the playing-state indicator at any given time

#### Scenario: Button pauses playback on second press when playing
- **WHEN** the user clicks the same "Play clip" button again while its clip is actively playing (`playingRowId === row.id && isPlaying`)
- **THEN** the system SHALL pause playback and the button SHALL revert to its idle appearance (Play icon, default blue style)

#### Scenario: Button shows failure state when playback cannot start
- **WHEN** playback for a clip fails to start (e.g., missing audio file, `get_meeting_audio_path` returns empty, `play()`/`playRange()` rejects, or `useAudioPlayer.error` becomes non-null)
- **THEN** the button for that row SHALL display a distinct failure indicator (e.g., red background/border, failure icon such as AlertCircle/XCircle) instead of the playing or idle state
- **THEN** the failure indicator SHALL clear when the user initiates a different playback or retries the same clip successfully

### Requirement: Bulk removal of all voiceprints and cached embeddings
The VoiceprintBrowser SHALL provide a single destructive "Remove all voiceprints & caches" action that, after explicit confirmation, deletes all rows from `speaker_embeddings` (both enrolled prototypes with `speaker_id` set and unassigned caches with `meeting_id`/`cluster_label`), and SHALL reflect the deletion immediately in the UI without requiring a page reload.

#### Scenario: Remove-all button is visible in Storage header
- **WHEN** the VoiceprintBrowser is rendered
- **THEN** a "Remove all voiceprints & caches" button (e.g., with Trash2 icon, destructive red styling) SHALL be visible in the Storage section header, disabled when `prototype_count + cache_count == 0` and enabled otherwise

#### Scenario: Confirmation dialog appears before deletion
- **WHEN** the user clicks "Remove all voiceprints & caches"
- **THEN** the system SHALL open a modal confirmation dialog that describes the destructive scope ("This will permanently delete N prototypes and M cached embeddings for all speakers/meetings and cannot be undone"), shows the current counts from `speaker_storage_stats`, and requires explicit Confirm vs Cancel

#### Scenario: Confirm deletes all embeddings and refreshes UI
- **WHEN** the user confirms in the dialog
- **THEN** the system SHALL invoke the bulk-clear command that executes `DELETE FROM speaker_embeddings` (clearing both prototypes and caches and any in-memory `PrototypeStore`), close the dialog, refresh `speaker_storage_stats` and `list_voiceprints` (so confirmed/unconfirmed lists become empty), clear any `playingRowId`/`failedRowId` playback state, pause any ongoing playback, and show a success notification

#### Scenario: Cancel leaves data untouched
- **WHEN** the user cancels or dismisses the confirmation dialog
- **THEN** no deletion SHALL occur, the dialog SHALL close, and the existing voiceprint lists and counts SHALL remain unchanged

#### Scenario: Dialog is inaccessible during empty store
- **WHEN** the store is already empty
- **THEN** the "Remove all" button SHALL be disabled and clicking it SHALL NOT open the confirmation dialog
