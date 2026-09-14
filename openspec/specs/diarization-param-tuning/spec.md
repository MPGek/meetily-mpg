# diarization-param-tuning Specification

## Purpose

Lets the offline diarization clustering behavior be tuned and measured: the merge threshold, speaker-count ceiling, and same-speaker gap-merge window are runtime parameters shared by the app pipeline and the evaluation harness, with a documented sweep protocol for choosing tuned defaults.

## Requirements

### Requirement: Clustering parameters are configurable at runtime
The offline diarization pipeline SHALL resolve its clustering parameters — clusterer kind (fixed-threshold `ahc`, built-in default; automatic-count `nmesc` or `vbx` selectable), merge threshold (minimum cosine similarity to merge two clusters; applies to the `ahc` kind only), speaker-count ceiling, and same-speaker gap-merge window — from persisted application settings, falling back to built-in defaults when no override is stored. The `ahc` default was selected by the 6.2 sweep (NME-SC under-clusters dense TitaNet windows: dev Conf 37.27 vs AHC 14.80). Changing a parameter SHALL NOT require a rebuild of the app or the harness binary. When the resolved kind is not `ahc`, a stored merge-threshold override SHALL have no effect on clustering. The `vbx` kind SHALL fail with an actionable error whenever the resolved embedding model family is not PLDA-compatible (the bundled enhanced TitaNet-Large family is 192-d; the vendored PLDA parameters require 256-d embeddings) — no panic, no silent kind switch.

#### Scenario: Stored override takes effect
- **WHEN** a user stores a merge-threshold override in application settings, selects the `ahc` clusterer kind, and triggers offline diarization
- **THEN** the clustering step uses the stored value and the resulting speaker blocks reflect it

#### Scenario: Clusterer kind switches without rebuild
- **WHEN** a user stores `ahc` as the clusterer kind override after the automatic-count default shipped
- **THEN** subsequent offline diarization runs fixed-threshold agglomerative clustering with the configured merge threshold, restoring pre-adoption behavior without a code revert

#### Scenario: Merge threshold inert under automatic count
- **WHEN** the clusterer kind is `vbx` or `nmesc` and a merge-threshold override is stored
- **THEN** diarization SHALL run with automatic speaker-count selection and the stored threshold SHALL NOT change the result

#### Scenario: Unset parameters use built-in defaults
- **WHEN** offline diarization runs with no stored overrides
- **THEN** the pipeline uses the built-in default values and behaves identically across app and harness

### Requirement: Speaker-count ceiling is always enforced
Offline clustering SHALL never run without a speaker-count ceiling, for every clusterer kind. When the user requests a maximum speaker count, the effective ceiling SHALL be that value; otherwise the configured default ceiling applies. The effective ceiling SHALL be clamped to the clustering backend's supported maximum (255). The pipeline SHALL NOT produce more speaker labels than the effective ceiling for a single diarization pass of one channel.

#### Scenario: Fragmented long recording is capped
- **WHEN** offline diarization runs on a long recording whose segmentation yields many short, noisy fragments and no user max-speakers is set
- **THEN** the number of distinct speaker labels in the output does not exceed the default ceiling

#### Scenario: User max-speakers wins when smaller
- **WHEN** the user sets a maximum speaker count below the configured default ceiling
- **THEN** the effective ceiling equals the user's value

#### Scenario: Oversized stored ceiling is clamped
- **WHEN** a stored ceiling or user max-speakers exceeds 255
- **THEN** the effective ceiling is 255 and the clamp is logged

### Requirement: Same-speaker gap merge in output
The pipeline SHALL merge consecutive output segments assigned to the same speaker when the silence gap between them does not exceed the configured gap-merge window, leaving overlapping speech and cross-speaker boundaries intact.

#### Scenario: Short gap within one speaker is bridged
- **WHEN** two segments of the same speaker are separated by a gap shorter than the configured window
- **THEN** the diarization output contains a single continuous segment spanning the gap

#### Scenario: Cross-speaker boundary is not bridged
- **WHEN** two adjacent segments are assigned to different speakers
- **THEN** the boundary between them is preserved regardless of the gap-merge window

### Requirement: Harness parameter overrides without rebuild
The `diarize-eval` harness binary SHALL accept command-line overrides for the clusterer kind, merge threshold, speaker-count ceiling, and gap-merge window, applying them to the same production clustering code path as the app. With no override flags, harness results SHALL remain byte-identical to app-pipeline results for identical input (parity requirement preserved).

#### Scenario: Override changes clustering only
- **WHEN** the harness runs a recording with a different merge threshold via CLI flag
- **THEN** only the clustering step differs from a default run and segmentation/embedding results are unchanged

#### Scenario: Clusterer kind override matches app behavior
- **WHEN** the harness runs with `--clusterer=ahc` on a recording
- **THEN** its clustering output matches the app pipeline configured with the `ahc` clusterer kind for the same audio and model set

#### Scenario: Default flags keep parity
- **WHEN** the harness runs without override flags after this change
- **THEN** its output matches the app pipeline's output for the same audio and model set

### Requirement: Parameter sweep protocol
The evaluation tooling SHALL provide a sweep capability that runs the harness over a chosen dataset (or the regression subset) for a grid of parameter values and records per-candidate DER components (FA, Miss, Conf) with the effective parameter values. Tuning SHALL be performed on designated tuning data and reported on held-out validation data; the benchmark sets used for published baselines SHALL NOT be used for selecting parameter values.

#### Scenario: Grid sweep produces comparable results
- **WHEN** a sweep runs over a grid of merge thresholds on the tuning subset
- **THEN** the tooling emits one result row per candidate with DER components, sorted for comparison, without re-downloading or re-normalizing datasets

#### Scenario: Held-out validation reporting
- **WHEN** sweep results are reported
- **THEN** the report distinguishes tuning-set metrics from held-out validation metrics for each candidate

### Requirement: Tuned defaults and re-baselined regression gate
After the sweep completes, the built-in default values for the clustering parameters SHALL be updated to the selected tuned values, and the subset regression gate SHALL be re-baselined against them: the gate's expected metric ranges are recorded in the evaluation documentation and enforced on subsequent runs.

#### Scenario: Shipped defaults match tuned values
- **WHEN** the app or harness runs with no overrides after this change
- **THEN** the effective parameter values equal the tuned values selected by the sweep

#### Scenario: Gate detects clustering regressions
- **WHEN** a later change degrades subset metrics beyond the recorded ranges
- **THEN** the subset gate run fails with a clear message identifying the regressed source
