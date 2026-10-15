## ADDED Requirements

### Requirement: Clustering distinguishes distinct speakers

The system SHALL cluster speaker embeddings with a fixed cosine-similarity threshold calibrated to the Balanced profile (`0.45`), so that distinct speakers in the audio are assigned distinct speaker labels rather than being merged into a single cluster.

#### Scenario: Multi-speaker meeting produces distinct labels

- **WHEN** offline diarization runs on a recording containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and the matched transcripts SHALL be assigned distinct speaker IDs (e.g. `SPEAKER_00` and `SPEAKER_01`)

#### Scenario: Single-speaker recording stays single-labeled

- **WHEN** offline diarization runs on a recording containing a single speaker on a channel
- **THEN** all matched transcripts SHALL be assigned the same speaker ID rather than being split into multiple spurious labels

### Requirement: Max speakers setting caps cluster count

The system SHALL apply the user-configured `maxSpeakers` setting as a hard ceiling on the number of clusters produced during offline diarization, and SHALL use no ceiling when the setting is unset or zero.

#### Scenario: Max speakers set

- **WHEN** offline diarization runs and the `maxSpeakers` setting is a positive value N
- **THEN** the clustering SHALL produce at most N distinct speaker labels

#### Scenario: Max speakers unset

- **WHEN** offline diarization runs and the `maxSpeakers` setting is unset or zero
- **THEN** the clustering SHALL run without a ceiling and infer the speaker count automatically

### Requirement: Singleton cluster pruning

The system SHALL dissolve single-segment clusters by reassigning their segments to the nearest larger speaker cluster, preventing spurious fragment speakers from inflating the speaker count.

#### Scenario: Singleton fragment reassigned

- **WHEN** clustering produces a cluster containing fewer than two segments
- **THEN** the system SHALL reassign that cluster's segments to the nearest cluster with at least two segments

## MODIFIED Requirements

### Requirement: Speaker label assignment

The system SHALL assign speaker labels to transcript segments by matching diarization time ranges to transcript timestamps, filling short gaps with the nearest speaker.

#### Scenario: Overlap-based speaker matching

- **WHEN** diarization produces speaker turns with start/end times
- **THEN** each transcript segment SHALL be assigned the speaker whose time range has the maximum overlap with the segment's `audio_start_time` to `audio_end_time`

#### Scenario: Unmatched transcript segments

- **WHEN** a transcript segment has no overlapping diarization turn and cannot be gap-filled (the channel has no turns, or the nearest turn on a multi-speaker channel is beyond 30 seconds)
- **THEN** the segment's `speaker` SHALL remain NULL

#### Scenario: Gap-fill on single-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel's turns all belong to a single speaker
- **THEN** the segment SHALL be assigned that channel's single speaker

#### Scenario: Gap-fill bounded on multi-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel has multiple speakers
- **THEN** the segment SHALL be assigned the temporally nearest turn's speaker when that turn lies within 30 seconds, and SHALL remain NULL otherwise
