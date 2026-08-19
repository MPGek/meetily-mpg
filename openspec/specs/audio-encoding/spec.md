# audio-encoding Specification

## Purpose

Defines how recorded meeting audio is encoded to disk, balancing audible quality for voice content against storage footprint and stereo channel fidelity.

## Requirements

### Requirement: Recording file codec and container
The system SHALL save recorded meeting audio as AAC-LC audio in an MP4/M4A container at a 48 kHz sample rate.

#### Scenario: Encoded recording is AAC-LC MP4
- **WHEN** a recording completes and its audio file is written
- **THEN** the file SHALL be an MP4/M4A container holding AAC-LC audio at 48 kHz

### Requirement: Voice-appropriate encoding bitrate
The system SHALL encode saved recordings with variable bitrate (VBR) targeting approximately 96 kbps for stereo voice audio, instead of fixed 192 kbps constant bitrate, so storage per hour is reduced without audible quality loss for speech.

#### Scenario: Variable bitrate encoding
- **WHEN** a recording is saved
- **THEN** the AAC encoding SHALL use variable bitrate mode targeting approximately 96 kbps for stereo voice content

#### Scenario: Storage footprint of a one-hour meeting
- **WHEN** a one-hour voice meeting is recorded and saved
- **THEN** the resulting audio file SHALL consume no more than 50 MB (compared to ~86 MB at 192 kbps CBR)

### Requirement: Consistent stereo channel encoding
All saved-recording paths SHALL preserve the stereo channel layout (left = microphone, right = system audio) so no recording silently loses a channel.

#### Scenario: Legacy save path preserves stereo
- **WHEN** a recording is saved through the non-incremental save path
- **THEN** the audio file SHALL contain two channels, matching the incremental checkpoint path

#### Scenario: Incremental checkpoint path preserves stereo
- **WHEN** a recording is saved via incremental checkpointing and merged to `audio.mp4`
- **THEN** the final file SHALL contain two channels with microphone on the left and system audio on the right
