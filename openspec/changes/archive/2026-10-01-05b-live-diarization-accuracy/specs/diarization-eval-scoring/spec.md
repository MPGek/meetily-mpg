# Spec Delta

## ADDED Requirements

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
