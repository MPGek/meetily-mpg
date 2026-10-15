## Purpose

Defines the streaming-only quality metrics for the online diarization path — how much delay the live output carries, how unstable its speaker labels are, and how finely it fragments one speaker's speech — so the online pipeline can be evaluated and regression-gated on axes that batch DER cannot express.

## ADDED Requirements

### Requirement: Streaming event record

A run in online mode SHALL emit a per-recording streaming event record alongside its hypothesis, capturing for every turn the online processor publishes: recording-relative start and end times in audio time, the raw cluster label, the identity-resolution fields the processor attached, whether the turn was final or provisional, and its emission order. The record SHALL be self-sufficient for metric computation, carrying no dependency on the application, its database, or its registry. It SHALL also identify the mode, chunking policy, and model set that produced it.

#### Scenario: Record is sufficient to compute metrics
- **WHEN** a streaming event record is read back without access to the application
- **THEN** every metric defined by this capability can be computed from the record and the dataset's reference annotation alone

#### Scenario: Provisional and final emissions are distinguishable
- **WHEN** the online processor publishes a turn that is not yet final
- **THEN** the record distinguishes it from a final turn, so metrics can be computed both over all emissions and over final ones only

#### Scenario: Record identifies its provenance
- **WHEN** a streaming event record is produced
- **THEN** it names the mode, the chunking policy, and the model set, so its metrics can never be compared against a record from a different configuration by accident

### Requirement: Emission lag measurement

The tooling SHALL measure emission lag for each reference speaker turn as the audio-time difference between the start of the reference turn and the earliest emission that covers it, and SHALL report the distribution as at least median and 90th percentile. Lag SHALL be derived from recording-relative audio time and SHALL NOT be derived from wall-clock time. Lag, throughput, and DER SHALL be reported as separate figures, never folded into a single score.

#### Scenario: Lag measured for a covered turn
- **WHEN** a reference turn is covered by an emitted turn
- **THEN** the lag is the audio-time distance from the reference turn's start to that emission, and it contributes to the reported distribution

#### Scenario: Uncovered reference speech is not silently dropped
- **WHEN** reference speech is never covered by any emission within the recording
- **THEN** the tooling counts it as uncovered and reports the count rather than omitting it from the lag statistics

#### Scenario: Lag is machine-independent
- **WHEN** the same recording and configuration are processed on two machines with different speeds
- **THEN** the reported lag values are the same

### Requirement: Label stability and fragmentation metrics

The tooling SHALL compute, over a recording's emitted stream and over its finalized output, at least: label flip rate, the number of distinct runs emitted per reference speaker, and the rate of cluster-label switches within a single reference speaker's active speech. These metrics SHALL be reported per dataset as duration-weighted aggregates. Reported fragmentation SHALL distinguish runs that are separate rows in the live transcript from runs that are merged away in the finalized output.

#### Scenario: Flicker is visible
- **WHEN** one reference speaker's contiguous speech is emitted as many alternating cluster labels
- **THEN** the flip rate and the run count for that speaker both increase, and the dataset aggregate reflects it

#### Scenario: Live fragmentation is separated from the finalized result
- **WHEN** the live stream fragments a speaker more than the finalized output does
- **THEN** the report shows both figures so the difference between the live view's row count and the saved transcript's row count is explicit

#### Scenario: Metric is per reference speaker
- **WHEN** the tooling reports fragmentation
- **THEN** runs are attributed to the reference speaker they overlap, so a globally clean clustering that still splits one speaker cannot hide in the aggregate

### Requirement: Real-time factor measurement

The tooling SHALL report real-time factor for the online path as the ratio of processed audio duration to processing duration for a documented measurement run on a named machine, reported separately from lag and from DER. Real-time factor SHALL NOT be included in any portability or parity claim, and SHALL NOT be used as a pass/fail gate whose bound depends on the measuring machine's speed.

#### Scenario: Throughput reported separately
- **WHEN** online mode is measured
- **THEN** the host machine and the real-time factor are reported as their own figures, distinct from the lag distribution and the DER

#### Scenario: Fast machine does not mask a regression
- **WHEN** a machine faster than the documented measurement machine is used
- **THEN** no gated metric changes, because no gate is derived from real-time factor

### Requirement: Streaming metrics reporting and gating

Streaming metrics SHALL be written into the same per-run result as the run's DER, SHALL be computed against the same reference annotation and annotated regions as that DER, and SHALL be gateable per dataset with recorded bounds that name the metric and the mode. A metric that cannot be computed for a recording because the input is missing SHALL cause a reported failure rather than a silently omitted value.

#### Scenario: Metrics travel with the score
- **WHEN** a run is scored
- **THEN** the streaming metrics are stored in the same run result as the DER and the reference and UEM used are identical for both

#### Scenario: Gate names metric and mode
- **WHEN** a recorded bound is exceeded
- **THEN** the failure message names the dataset, the metric, the mode, and the measured value

#### Scenario: Missing input fails loudly
- **WHEN** an online run lacks the streaming event record needed to compute a metric
- **THEN** the command fails with a message naming the recording rather than reporting the metric as absent or zero
