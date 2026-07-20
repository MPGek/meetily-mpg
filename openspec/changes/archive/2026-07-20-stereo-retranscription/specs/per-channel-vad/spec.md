## ADDED Requirements

### Requirement: Independent VAD per channel
The system SHALL run independent voice activity detection on each audio channel to identify speech segments per source.

#### Scenario: VAD on stereo channels
- **WHEN** stereo audio has been extracted into separate left and right channels
- **THEN** the system SHALL run VAD on the left channel to identify microphone speech segments
- **AND** the system SHALL run VAD on the right channel to identify system audio speech segments
- **AND** each channel's VAD SHALL operate independently with its own state

#### Scenario: VAD on mono audio
- **WHEN** mono audio is being retranscribed
- **THEN** the system SHALL run a single VAD pass on the mono stream

### Requirement: Per-channel progress reporting
The system SHALL report VAD progress separately for each channel during stereo retranscription.

#### Scenario: Stereo VAD progress
- **WHEN** VAD is processing stereo audio
- **THEN** progress updates SHALL reflect the combined progress of both channel VAD passes
- **AND** the progress range SHALL be allocated proportionally (e.g., 20-25% for mic VAD, 25-30% for system VAD)
