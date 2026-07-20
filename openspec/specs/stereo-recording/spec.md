# stereo-recording Specification

## Purpose
Stereo audio recording with channel mapping for microphone and system audio.

## Requirements
### Requirement: Stereo audio recording with channel mapping
The system SHALL save the final recording as a stereo audio file with microphone audio on the left channel and system audio on the right channel.

#### Scenario: Recording saved as stereo MP4
- **WHEN** a recording session completes
- **THEN** the saved `audio.mp4` file SHALL have 2 audio channels in AAC format

#### Scenario: Left channel contains microphone audio
- **WHEN** only microphone captures audio and system audio is silent during recording
- **THEN** the left channel of the saved recording SHALL contain the microphone audio and the right channel SHALL contain silence

#### Scenario: Right channel contains system audio
- **WHEN** only system audio captures sound and microphone is silent during recording
- **THEN** the right channel of the saved recording SHALL contain the system audio and the left channel SHALL contain silence

#### Scenario: Both channels active
- **WHEN** both microphone and system audio capture sound simultaneously
- **THEN** the left channel SHALL contain microphone audio and the right channel SHALL contain system audio, with no cross-contamination between channels

### Requirement: Stereo pipeline chunks
The system SHALL carry stereo-interleaved audio data for recording chunks flowing through the pipeline, with each `AudioChunk` tagged with `channels: 2`.

#### Scenario: Recording chunk is stereo
- **WHEN** the pipeline emits an audio chunk for the recording saver
- **THEN** the chunk SHALL have `channels: 2` and contain interleaved `[L,R,L,R,...]` samples at the pipeline sample rate

#### Scenario: VAD segment remains mono
- **WHEN** a VAD processor emits a speech segment for transcription
- **THEN** the transcription chunk SHALL have `channels: 1` and contain mono samples at 16kHz
