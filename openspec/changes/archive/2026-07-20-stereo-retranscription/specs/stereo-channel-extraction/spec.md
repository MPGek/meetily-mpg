## ADDED Requirements

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
