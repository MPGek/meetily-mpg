## ADDED Requirements

### Requirement: Play button on transcript blocks in meeting details view
Transcript blocks (utterances) in the meeting details view SHALL show a play button that seeks the meeting audio player to the block's recording-relative start time and resumes playback, enabling validation of transcription text and speaker assignment.

#### Scenario: Play button shown for timed utterances
- **WHEN** a transcript block has an `audio_start_time` and the meeting has a resolvable audio file
- **THEN** the block SHALL display a play button (in all three visual variants: legacy, microphone, system)

#### Scenario: Play button hidden without audio time
- **WHEN** a transcript block has no `audio_start_time`
- **THEN** the block SHALL NOT display a play button

#### Scenario: Play button hidden when meeting has no audio
- **WHEN** the meeting has no resolvable audio file
- **THEN** no transcript block SHALL display a play button

#### Scenario: Activating play button seeks and resumes playback
- **WHEN** the user clicks the play button on a transcript block
- **THEN** the audio player SHALL seek to the block's `audio_start_time` and resume playback
