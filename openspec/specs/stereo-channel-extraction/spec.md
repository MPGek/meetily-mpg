# stereo-channel-extraction Specification

## Purpose
Extract and resample individual audio channels from stereo recordings for independent processing.

## Requirements
### Requirement: Stereo channel extraction
The system SHALL extract left (microphone) and right (system) channels from stereo audio recordings as separate mono streams for independent processing.

#### Scenario: Extract channels from stereo audio
- **WHEN** a stereo audio file (2 channels) is decoded for retranscription
- **THEN** the system SHALL extract the left channel as a mono stream and the right channel as a separate mono stream

#### Scenario: Handle mono audio gracefully
- **WHEN** a mono audio file (1 channel) is decoded for retranscription
- **THEN** the system SHALL treat it as a single unknown source and process it through the mono path

### Requirement: Per-channel resampling
The system SHALL resample each extracted channel to 16kHz independently for VAD and transcription.

#### Scenario: Resample stereo channels
- **WHEN** stereo channels are extracted from audio at a sample rate other than 16kHz
- **THEN** each channel SHALL be resampled to 16kHz independently using the existing high-quality sinc resampler

### Requirement: Channel-layout detection from decoded audio
The system SHALL determine the channel layout used for per-channel processing from the actually decoded audio, not from container/header metadata alone. When the decoded audio has two channels the layout SHALL be treated as stereo (left=microphone, right=system); when metadata lacks channel information the system SHALL NOT downgrade the layout to mono. A single-channel layout SHALL be used only when the decoded audio genuinely has one channel.

#### Scenario: Metadata missing but audio is stereo
- **WHEN** an audio file's container metadata does not expose a channel count but decoding yields two channels
- **THEN** the system SHALL treat the file as stereo for channel extraction and per-channel processing

#### Scenario: Metadata disagrees with decoded audio
- **WHEN** container metadata reports a channel count that disagrees with the decoded audio
- **THEN** the decoded channel layout SHALL take precedence for extraction and downstream processing
