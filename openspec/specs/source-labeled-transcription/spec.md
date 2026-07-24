# source-labeled-transcription Specification

## Purpose
Transcription segments carry source device metadata to distinguish microphone from system audio.

## Requirements
### Requirement: Transcription source labeling
The system SHALL include source device metadata in each transcript update event, identifying whether the transcribed speech came from the microphone or system audio. The transcription worker SHALL pass the previous transcript text as a Whisper context prompt for the next segment from the same source device.

#### Scenario: Microphone transcription labeled
- **WHEN** a VAD speech segment from the microphone is successfully transcribed
- **THEN** the emitted `TranscriptUpdate` SHALL have `source_device` set to `"Microphone"`
- **AND** the transcription worker SHALL cache the transcript text for use as context prompt for the next microphone segment

#### Scenario: System audio transcription labeled
- **WHEN** a VAD speech segment from system audio is successfully transcribed
- **THEN** the emitted `TranscriptUpdate` SHALL have `source_device` set to `"System"`
- **AND** the transcription worker SHALL cache the transcript text for use as context prompt for the next system audio segment

#### Scenario: Source device persisted in transcripts
- **WHEN** transcription results are saved to `transcripts.json`
- **THEN** each `TranscriptSegment` SHALL include the `source_device` field

#### Scenario: Previous text passed as Whisper prompt
- **WHEN** a new VAD segment from source X arrives at the transcription worker
- **THEN** the worker SHALL include the last transcript text from the same source X as `initial_prompt` in the Whisper `FullParams`
- **AND** if no previous transcript exists for source X, the `initial_prompt` SHALL be empty

#### Scenario: Prompt resets on recording boundary
- **WHEN** a new recording session starts
- **THEN** all cached previous-text prompts for both microphone and system SHALL be cleared
