# diarization-eval-scoring Specification

## Purpose

Defines how hypothesis RTTMs are scored against reference annotations and reported, using the community-standard DER setup so results are directly comparable with published baselines.

## Requirements

### Requirement: Full-setup DER scoring
The scoring command SHALL compute Diarization Error Rate per dataset with no forgiveness collar and overlapped speech counted, restricted to UEM-annotated regions, decomposed into false-alarm, missed-detection, and speaker-confusion components.

#### Scenario: Score a completed run
- **WHEN** the score command is invoked for a dataset that has both reference and hypothesis RTTMs
- **THEN** it reports overall DER and the FA/Miss/Conf breakdown for that dataset

#### Scenario: Missing hypotheses
- **WHEN** the score command is invoked but some recordings lack hypothesis RTTMs
- **THEN** it fails, listing the missing recordings, and does not report a partial score as if complete

### Requirement: Baseline comparison report
The system SHALL produce a Markdown report comparing each dataset's measured DER against the published pyannote 3.1 baseline for that dataset, stored at a documented path, and the baseline values SHALL be declared per dataset in configuration rather than hardcoded in logic.

#### Scenario: Report after scoring
- **WHEN** scoring finishes for one or more datasets
- **THEN** the report contains one row per scored dataset with DER, FA, Miss, Conf, the baseline DER, and the delta

### Requirement: Fast regression subset
The tooling SHALL define a small fixed subset (at most 10 recordings, drawn from the Russian synthetic set and VoxConverse) that can be downloaded, diarized, and scored with a single command, intended as a repeatable regression gate.

#### Scenario: Regression gate run
- **WHEN** the subset command is run on a machine that has completed the one-time subset preparation
- **THEN** it completes end-to-end and prints subset DER per source dataset without requiring gated data

#### Scenario: Gate detects clustering regression
- **WHEN** a threshold or clustering change worsens subset DER beyond a documented tolerance
- **THEN** the command output makes the per-component delta visible so the regression is attributable
