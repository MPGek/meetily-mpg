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

### Requirement: Speaker attribution survives retranscription
Retranscription SHALL NOT leave a meeting with silently discarded speaker attribution or with cluster bindings that no longer correspond to any transcript. After a successful retranscription the meeting SHALL re-establish speaker attribution and provenance for the new transcript rows: user-confirmed identities SHALL be carried forward where their time ranges overlap and/or channel-correct diarization SHALL be re-run, so that the transcript presents the same speaker names, `matched_by` provenance, and match scores as a meeting that was diarized correctly. A user-confirmed (`matched_by='user'`) identity SHALL NOT be lost by retranscription.

#### Scenario: Retranscription of a meeting with confirmed speakers
- **WHEN** retranscription completes for a meeting whose transcripts had user-confirmed speaker identities
- **THEN** the new transcript rows SHALL display those identities for the overlapping time ranges with user provenance, without the user re-assigning them

#### Scenario: Retranscription leaves no stale bindings
- **WHEN** retranscription completes and re-analysis has not yet run
- **THEN** the meeting SHALL NOT present cluster bindings whose labels no longer map to any transcript as if they applied to the new rows, and the user-visible labels, confidence values, and confirmation affordances SHALL be consistent with the attribution actually recorded
