## ADDED Requirements

### Requirement: Transcription source labeling
The system SHALL include source device metadata in each transcript update event, identifying whether the transcribed speech came from the microphone or system audio.

#### Scenario: Microphone transcription labeled
- **WHEN** a VAD speech segment from the microphone is successfully transcribed
- **THEN** the emitted `TranscriptUpdate` SHALL have `source_device` set to `"Microphone"`

#### Scenario: System audio transcription labeled
- **WHEN** a VAD speech segment from system audio is successfully transcribed
- **THEN** the emitted `TranscriptUpdate` SHALL have `source_device` set to `"System"`

#### Scenario: Source device persisted in transcripts
- **WHEN** transcription results are saved to `transcripts.json`
- **THEN** each `TranscriptSegment` SHALL include the `source_device` field
