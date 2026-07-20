# source-labeled-retranscription Specification

## Purpose
Source-labeled transcription for retranscribed audio with per-channel processing.

## Requirements
### Requirement: Source-labeled transcription
The system SHALL transcribe each channel's speech segments separately and label results with source_device metadata.

#### Scenario: Transcribe microphone segments
- **WHEN** VAD has identified speech segments on the left (microphone) channel
- **THEN** the system SHALL transcribe each segment using the configured transcription engine
- **AND** each resulting TranscriptSegment SHALL have `source_device` set to `"Microphone"`

#### Scenario: Transcribe system segments
- **WHEN** VAD has identified speech segments on the right (system) channel
- **THEN** the system SHALL transcribe each segment using the configured transcription engine
- **AND** each resulting TranscriptSegment SHALL have `source_device` set to `"System"`

#### Scenario: Transcribe mono segments
- **WHEN** VAD has identified speech segments on a mono audio stream
- **THEN** the system SHALL transcribe each segment
- **AND** each resulting TranscriptSegment SHALL have `source_device` set to `None`

### Requirement: Result merging and sorting
The system SHALL merge transcription results from both channels into a single chronologically sorted list.

#### Scenario: Merge stereo results
- **WHEN** both microphone and system channels have been transcribed
- **THEN** the system SHALL merge all TranscriptSegments into a single list
- **AND** the list SHALL be sorted by `audio_start_time` in ascending order

### Requirement: Cancellation support
The system SHALL support cancellation during stereo retranscription, including during per-channel VAD and transcription.

#### Scenario: Cancel during stereo retranscription
- **WHEN** a cancellation is requested during stereo retranscription
- **THEN** the system SHALL stop processing both channels
- **AND** the system SHALL return an error indicating cancellation
