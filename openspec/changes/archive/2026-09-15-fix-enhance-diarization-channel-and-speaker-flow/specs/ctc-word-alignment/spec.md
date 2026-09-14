## ADDED Requirements

### Requirement: Per-channel refinement uses the decoded channel layout
When word-level refinement reads a meeting's saved audio per channel, the system SHALL determine whether the audio is stereo from the actually decoded audio, not from container/header metadata, so refined token times are mapped to the same microphone or system channel the segment was transcribed from.

#### Scenario: Refining a segment from a stereo recording with unknown metadata
- **WHEN** offline repair refines word tokens for a meeting whose saved audio has two decoded channels but no channel count in its metadata
- **THEN** refinement SHALL read the segment's audio span from its `source_device` channel and SHALL NOT collapse both channels into one

#### Scenario: Mono meeting refinement
- **WHEN** offline repair refines word tokens for a meeting whose decoded audio has a single channel
- **THEN** refinement SHALL read the span from that single stream
