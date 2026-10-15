# diarization-eval-scoring Specification

## Purpose

Defines how hypothesis RTTMs are scored against reference annotations and reported, using the community-standard DER setup so results are directly comparable with published baselines.

## Requirements

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

### Requirement: Live-versus-final label agreement
The tooling SHALL report, for an online run, how often the speaker label a user would have seen during the recording differs from the label the same speech carries in the finalized output. The measure SHALL be computed over finalized speech, comparing for each unit of finalized speech the last label emitted for it while streaming against its label in the finalized result, after the same optimal label mapping used for the run's DER so a pure renaming of clusters is not counted as a disagreement. Finalized speech that no emission ever covered SHALL be counted and reported separately rather than folded into the agreement figure or silently dropped. The measure SHALL be computed from the run's own streaming event record and finalized output, against the same reference and annotated regions as that run's DER, and SHALL be stored in the same run result.

#### Scenario: A stable run scores near zero disagreement
- **WHEN** an online run's emitted labels match its finalized labels for all covered speech
- **THEN** the reported disagreement rate SHALL be zero and the uncovered count SHALL be reported alongside it

#### Scenario: An end-of-meeting relabel is counted
- **WHEN** the finalized output assigns a different speaker than the last live emission for part of the speech
- **THEN** that speech SHALL count toward the disagreement rate in proportion to its duration

#### Scenario: Renaming clusters is not a disagreement
- **WHEN** the finalized output uses different cluster names for exactly the same partition of speech as the live stream
- **THEN** the disagreement rate SHALL be zero, because the same optimal mapping as the run's DER is applied first

#### Scenario: Never-covered speech is reported, not hidden
- **WHEN** part of the finalized speech was never covered by any live emission
- **THEN** it SHALL be reported as an uncovered quantity and SHALL NOT be counted as agreement

#### Scenario: Missing streaming record fails loudly
- **WHEN** an online run lacks the streaming event record needed for this measure
- **THEN** the command SHALL fail naming the recording rather than reporting a zero or omitted value

### Requirement: Online accuracy bounds are recorded and gated
The regression gate SHALL accept recorded per-dataset bounds for the online path covering, at minimum, how far the online result may fall behind the offline result on the same dataset and how much live-versus-final disagreement is tolerated. Each bound SHALL name its dataset, its metric, and the mode it constrains, and exceeding one SHALL fail the gate with those three named together with the measured value. For a dataset whose absolute error is known to be inflated by an annotation artifact, the recorded bound SHALL constrain the component that is meaningful for that dataset rather than the inflated total. No bound SHALL be derived from a throughput measurement whose value depends on the measuring machine.

#### Scenario: Online regression fails the gate
- **WHEN** a change makes the online result fall further behind the offline result on a gated dataset than the recorded bound allows
- **THEN** the gate SHALL fail, naming the dataset, the metric, the mode, and the measured value

#### Scenario: Disagreement regression fails the gate
- **WHEN** a change raises live-versus-final disagreement above the recorded bound
- **THEN** the gate SHALL fail with that metric named

#### Scenario: Artifact-inflated dataset is gated on the meaningful component
- **WHEN** a dataset's absolute error is inflated by a known annotation artifact
- **THEN** its recorded online bound SHALL constrain the component that the artifact does not inflate

#### Scenario: Throughput is never a gate
- **WHEN** the online path is measured on a faster or slower machine
- **THEN** no gated bound SHALL change, because no bound is derived from the throughput figure
