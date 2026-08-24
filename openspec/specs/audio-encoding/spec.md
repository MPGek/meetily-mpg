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

### Requirement: Encode input sample sanitization
The system SHALL replace non-finite sample values (NaN and ±Infinity) with silence (0.0) in audio data before encoding to disk, so the AAC encoder never fails a recording save because of malformed samples.

#### Scenario: Audio contains a NaN window
- **WHEN** the PCM data being encoded contains non-finite samples
- **THEN** the encoder SHALL receive the data with non-finite samples replaced by 0.0 and SHALL produce a valid output file (silence in the malformed region)

#### Scenario: Checkpoint encode never aborts on malformed input
- **WHEN** a recording checkpoint contains NaN or infinite samples
- **THEN** checkpoint encoding SHALL complete with the malformed region encoded as silence rather than failing the entire save

### Requirement: Checkpoint interval accounting
The system SHALL size incremental checkpoints in audio frames per channel so each checkpoint corresponds to the intended ~30 seconds of recorded session audio regardless of stereo interleaving.

#### Scenario: Stereo checkpoint timing
- **WHEN** stereo-interleaved samples are accumulated for checkpointing
- **THEN** a checkpoint SHALL be written approximately every 30 seconds of session audio, not every ~15 seconds as occurs when interleaved stereo samples are counted as mono frames

#### Scenario: Checkpoint count for a long meeting
- **WHEN** a ~60 minute stereo meeting is saved incrementally
- **THEN** the number of checkpoints merged into the final file SHALL be roughly 120 (30-second intervals), avoiding doubled checkpoint counts
