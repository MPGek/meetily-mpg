## ADDED Requirements

### Requirement: Channel-layout detection from decoded audio
The system SHALL determine the channel layout used for per-channel processing from the actually decoded audio, not from container/header metadata alone. When the decoded audio has two channels the layout SHALL be treated as stereo (left=microphone, right=system); when metadata lacks channel information the system SHALL NOT downgrade the layout to mono. A single-channel layout SHALL be used only when the decoded audio genuinely has one channel.

#### Scenario: Metadata missing but audio is stereo
- **WHEN** an audio file's container metadata does not expose a channel count but decoding yields two channels
- **THEN** the system SHALL treat the file as stereo for channel extraction and per-channel processing

#### Scenario: Metadata disagrees with decoded audio
- **WHEN** container metadata reports a channel count that disagrees with the decoded audio
- **THEN** the decoded channel layout SHALL take precedence for extraction and downstream processing
