## MODIFIED Requirements

### Requirement: Clustering parameters are configurable at runtime
The offline diarization pipeline SHALL resolve its clustering parameters — clusterer kind (automatic-count `nmesc` or `vbx`; fixed-threshold `ahc`; built-in default `nmesc`), merge threshold (minimum cosine similarity to merge two clusters; applies to the `ahc` kind only), speaker-count ceiling, and same-speaker gap-merge window — from persisted application settings, falling back to built-in defaults when no override is stored. Changing a parameter SHALL NOT require a rebuild of the app or the harness binary. When the resolved kind is not `ahc`, a stored merge-threshold override SHALL have no effect on clustering. The `vbx` kind SHALL fail with an actionable error whenever the resolved embedding model family is not PLDA-compatible (the bundled enhanced TitaNet-Large family is 192-d; the vendored PLDA parameters require 256-d embeddings) — no panic, no silent kind switch.

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
