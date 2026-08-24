## ADDED Requirements

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