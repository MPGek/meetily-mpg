# vad-config Specification

## Purpose
Centralized Voice Activity Detection parameter management with mode-specific presets (live vs batch) and segment merging for batch processing.

## Requirements

### Requirement: Centralized VAD configuration
The system SHALL define a `VadConfig` struct containing all VAD parameters (thresholds, padding, redemption, segment limits) and SHALL pass it to `ContinuousVadProcessor::new()` instead of a single `redemption_time_ms` parameter.

#### Scenario: Live mode preset
- **WHEN** `VadConfig::live()` is called
- **THEN** it SHALL return a config with redemption=200ms, pre_pad=150ms, post_pad=150ms, min_speech=250ms, threshold=0.50, neg_threshold=0.35, and no maximum segment limit

#### Scenario: Batch mode preset
- **WHEN** `VadConfig::batch()` is called
- **THEN** it SHALL return a config with redemption=200ms, pre_pad=150ms, post_pad=150ms, min_speech=250ms, threshold=0.50, neg_threshold=0.35, and a maximum segment of 25 seconds (400,000 samples at 16kHz)

#### Scenario: Parameter passing to VAD processor
- **WHEN** `ContinuousVadProcessor::new(sample_rate, config)` is called with a `VadConfig`
- **THEN** the processor SHALL use all values from the config struct for its internal thresholds, padding, and redemption calculations

### Requirement: Segment merger for batch processing
The system SHALL provide a `merge_segments` function that combines adjacent VAD segments where the inter-segment gap is less than a configurable threshold, and splits merged segments exceeding a maximum duration.

#### Scenario: Adjacent segments merged
- **WHEN** two speech segments have a gap < 2000ms between `end` of the first and `start` of the second
- **THEN** the segments SHALL be combined into a single segment spanning from the first segment's start to the second segment's end

#### Scenario: Distant segments kept separate
- **WHEN** two speech segments have a gap ≥ 2000ms between `end` of the first and `start` of the second
- **THEN** the segments SHALL remain as separate segments

#### Scenario: Merged segment exceeds max duration
- **WHEN** a merged segment exceeds the configured maximum duration (25 seconds)
- **THEN** it SHALL be split at the largest silence gap within the segment
- **AND** each resulting sub-segment SHALL not exceed the maximum duration

### Requirement: No duplicate VAD redemption constants
The system SHALL NOT define duplicate `VAD_REDEMPTION_TIME_MS` constants across `retranscription.rs`, `import.rs`, and `pipeline.rs`. All VAD configuration SHALL source from `VadConfig` presets.

#### Scenario: Retranscription uses batch config
- **WHEN** retranscription initializes VAD
- **THEN** it SHALL use `VadConfig::batch()` instead of a locally-defined `VAD_REDEMPTION_TIME_MS` constant

#### Scenario: Import uses batch config
- **WHEN** import initializes VAD
- **THEN** it SHALL use `VadConfig::batch()` instead of a locally-defined `VAD_REDEMPTION_TIME_MS` constant

#### Scenario: Live pipeline uses live config
- **WHEN** the live audio pipeline initializes VAD processors
- **THEN** it SHALL use `VadConfig::live()` instead of a hardcoded redemption value
