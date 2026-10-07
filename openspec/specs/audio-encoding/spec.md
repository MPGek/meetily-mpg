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

### Requirement: Merging checkpoints keeps the recorded sample count
When checkpoints are merged into the final recording, the system SHALL produce audio whose length and timing equal the concatenated checkpoint audio. It SHALL NOT insert encoder delay or padding at checkpoint boundaries. Over a whole recording, the merged file SHALL differ from the checkpoint audio by at most one encoder frame (about 21 ms).

#### Scenario: One-hour recording
- **WHEN** a 60-minute recording made of about 120 checkpoints is finalized
- **THEN** the merged file's duration SHALL be within 25 ms of the audio delivered to the saver, and a sound delivered at 59:00 SHALL play at 59:00 within 25 ms

#### Scenario: Sound right after a checkpoint boundary
- **WHEN** a short sound is recorded just after the boundary between two checkpoints
- **THEN** it SHALL play at the time it was recorded, with no added silence or delay at the boundary

### Requirement: Crash recovery merges checkpoints the same way
When audio is recovered from the checkpoints of an interrupted recording, the recovered file SHALL keep the recorded sample count by the same rule as a normal finalize. Checkpoint folders written by earlier versions SHALL remain recoverable.

#### Scenario: Recovering an interrupted recording
- **WHEN** the app crashes during a recording and the user recovers its audio
- **THEN** the recovered file SHALL keep the checkpointed audio's timing, with no added delay at checkpoint boundaries

#### Scenario: Recovering a folder left by an earlier version
- **WHEN** the user recovers a recording whose checkpoints were written by an earlier version of the app
- **THEN** recovery SHALL still produce an audio file from those checkpoints
