## MODIFIED Requirements

### Requirement: Full-setup DER scoring

The scoring command SHALL compute Diarization Error Rate per dataset with no forgiveness collar and overlapped speech counted, restricted to UEM-annotated regions, decomposed into false-alarm, missed-detection, and speaker-confusion components. The same setup SHALL be applied to a run produced by either mode, and a scored result SHALL always identify the mode that produced the hypothesis.

#### Scenario: Score a completed run
- **WHEN** the score command is invoked for a dataset that has both reference and hypothesis RTTMs
- **THEN** it reports overall DER and the FA/Miss/Conf breakdown for that dataset

#### Scenario: Missing hypotheses
- **WHEN** the score command is invoked but some recordings lack hypothesis RTTMs
- **THEN** it fails, listing the missing recordings, and does not report a partial score as if complete

#### Scenario: Score an online run
- **WHEN** the score command is invoked for a run produced in online mode
- **THEN** it applies the identical no-collar, overlap-counted setup, restricted to the same UEM regions, and reports the result attributed to that run's mode

#### Scenario: Modes are never mixed into one score
- **WHEN** a dataset has hypotheses from more than one mode
- **THEN** a score is computed per mode and never as a combined figure

### Requirement: Baseline comparison report

The system SHALL produce a Markdown report comparing each dataset's measured DER against the published pyannote 3.1 baseline for that dataset, stored at a documented path, and the baseline values SHALL be declared per dataset in configuration rather than hardcoded in logic. For datasets that have both an offline and an online run, the report SHALL present both results side by side together with the streaming metric columns, so the cost of streaming versus batch processing is visible per dataset.

#### Scenario: Report after scoring
- **WHEN** scoring finishes for one or more datasets
- **THEN** the report contains one row per scored dataset with DER, FA, Miss, Conf, the baseline DER, and the delta

#### Scenario: Report shows both modes
- **WHEN** a dataset has both an offline and an online scored run
- **THEN** the report shows both DER results with the online-minus-offline delta and the streaming metric columns for that dataset

#### Scenario: Single-mode dataset stays readable
- **WHEN** a dataset has only one mode scored
- **THEN** the report shows that mode's result without implying a missing comparison

### Requirement: Fast regression subset

The tooling SHALL define a small fixed subset (at most 10 recordings, drawn from the Russian synthetic set and VoxConverse) that can be downloaded, diarized, and scored with a single command, intended as a repeatable regression gate. The gate SHALL accept online-mode metrics alongside the offline ones, and every recorded bound SHALL state which mode and which metric it constrains.

#### Scenario: Regression gate run
- **WHEN** the subset command is run on a machine that has completed the one-time subset preparation
- **THEN** it completes end-to-end and prints subset DER per source dataset without requiring gated data

#### Scenario: Gate detects clustering regression
- **WHEN** a threshold or clustering change worsens subset DER beyond a documented tolerance
- **THEN** the command output makes the per-component delta visible so the regression is attributable

#### Scenario: Gate covers the online path
- **WHEN** the subset command runs in online mode
- **THEN** it evaluates the recorded online bounds (including the streaming metrics) and fails with a message naming the mode, the metric, and the dataset when a bound is exceeded
