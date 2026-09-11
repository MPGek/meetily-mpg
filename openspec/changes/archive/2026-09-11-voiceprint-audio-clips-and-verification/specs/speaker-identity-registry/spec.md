## MODIFIED Requirements

### Requirement: Voiceprint storage visibility
The system SHALL provide voiceprint storage statistics (registry speaker count, enrolled prototype count, unassigned cache count, total embedding bytes) via a command, and SHALL display them in Settings so the user can track growth.

#### Scenario: Stats displayed
- **WHEN** user opens the speaker/diarization section of Settings
- **THEN** the UI SHALL show the current voiceprint storage usage (counts and human-readable size)

## ADDED Requirements

### Requirement: Audio clip bytes reported separately
The storage statistics command SHALL additionally report total stored audio-clip bytes and the count of rows carrying a clip, separately from embedding bytes. The Settings display SHALL show audio storage alongside embedding storage using the same human-readable size formatting.

#### Scenario: Audio bytes shown separately
- **WHEN** the browser loads with embedding bytes and audio-clip bytes present
- **THEN** the summary SHALL show both sizes distinctly rather than merging them into one number

#### Scenario: Zero audio shows zero
- **WHEN** no voiceprint row carries an audio clip
- **THEN** the audio portion of the summary SHALL show zero without error
