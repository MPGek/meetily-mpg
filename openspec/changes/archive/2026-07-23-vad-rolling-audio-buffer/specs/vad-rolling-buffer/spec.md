# vad-rolling-buffer Specification

## Purpose
Rolling audio buffer in VAD processor for backfilling speech onset audio when speech is detected.

## Requirements

### Requirement: Rolling audio buffer maintenance
The system SHALL maintain a rolling audio buffer in `ContinuousVadProcessor` that stores the most recent processed audio windows.

#### Scenario: Buffer initialized with capacity
- **WHEN** `ContinuousVadProcessor` is created
- **THEN** it SHALL initialize an audio history buffer with capacity for 10 windows (5120 samples at 16kHz = 320ms)

#### Scenario: Buffer updated after each window
- **WHEN** a 512-sample window is processed in `process_chunk`
- **THEN** the window SHALL be appended to the audio history buffer
- **AND** if the buffer exceeds capacity, the oldest samples SHALL be removed to maintain the fixed size

### Requirement: Speech onset recovery via buffer backfill
The system SHALL prepend audio from the rolling buffer to `current_speech` when speech is detected, recovering the speech onset that was previously lost.

#### Scenario: Speech detected with buffer available
- **WHEN** VAD probability crosses the positive threshold (0.50) and the audio history buffer contains samples
- **THEN** the system SHALL prepend the buffered audio to `current_speech` before adding the current detection window
- **AND** the segment SHALL include the real audio from the buffer (not zeros)

#### Scenario: Speech detected with empty buffer
- **WHEN** VAD probability crosses the positive threshold (0.50) and the audio history buffer is empty
- **THEN** the system SHALL start `current_speech` with only the current detection window
- **AND** the behavior SHALL match the previous implementation (no backfill)

### Requirement: Buffer capacity configuration
The system SHALL allow the buffer capacity to be configured via `VadConfig`, with a default of 10 windows.

#### Scenario: Default buffer capacity
- **WHEN** `VadConfig::live()` or `VadConfig::batch()` is called
- **THEN** the buffer capacity SHALL be set to 10 windows (5120 samples)

#### Scenario: Custom buffer capacity
- **WHEN** a custom `VadConfig` is created with a different buffer capacity
- **THEN** the `ContinuousVadProcessor` SHALL use the specified capacity
