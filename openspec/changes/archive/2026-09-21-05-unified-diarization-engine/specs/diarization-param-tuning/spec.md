# Spec Delta

## MODIFIED Requirements

### Requirement: Clustering parameters are configurable at runtime
The diarization pipeline SHALL resolve its clustering parameters — clusterer kind (fixed-threshold `ahc`, built-in default; automatic-count `nmesc` or `vbx` selectable), merge threshold (minimum cosine similarity to merge two clusters; applies to the `ahc` kind only), speaker-count ceiling, and same-speaker gap-merge window — from persisted application settings, falling back to built-in defaults when no override is stored. The resolved parameters SHALL apply to every clustering pass the application performs, whether it runs on a saved recording after the fact or on a session's buffered speech while or just after recording, so a single stored setting governs both. The `ahc` default was selected by the 6.2 sweep (NME-SC under-clusters dense TitaNet windows: dev Conf 37.27 vs AHC 14.80). Changing a parameter SHALL NOT require a rebuild of the app or the harness binary. When the resolved kind is not `ahc`, a stored merge-threshold override SHALL have no effect on clustering. The `vbx` kind SHALL fail with an actionable error whenever the resolved embedding model family is not PLDA-compatible (the bundled enhanced TitaNet-Large family is 192-d; the vendored PLDA parameters require 256-d embeddings) — no panic, no silent kind switch.

#### Scenario: Stored override takes effect
- **WHEN** a user stores a merge-threshold override in application settings, selects the `ahc` clusterer kind, and triggers offline diarization
- **THEN** the clustering step uses the stored value and the resulting speaker blocks reflect it

#### Scenario: Stored override reaches live diarization
- **WHEN** a user stores a merge-threshold or clusterer-kind override and then makes a recording with online diarization enabled
- **THEN** the session's clustering SHALL use the stored values, and a recording made with a deliberately extreme stored merge threshold SHALL produce a visibly different number of speaker labels than one made with the default

#### Scenario: Clusterer kind switches without rebuild
- **WHEN** a user stores `ahc` as the clusterer kind override after the automatic-count default shipped
- **THEN** subsequent offline diarization runs fixed-threshold agglomerative clustering with the configured merge threshold, restoring pre-adoption behavior without a code revert

#### Scenario: Merge threshold inert under automatic count
- **WHEN** the clusterer kind is `vbx` or `nmesc` and a merge-threshold override is stored
- **THEN** diarization SHALL run with automatic speaker-count selection and the stored threshold SHALL NOT change the result

#### Scenario: Unset parameters use built-in defaults
- **WHEN** offline diarization runs with no stored overrides
- **THEN** the pipeline uses the built-in default values and behaves identically across app and harness

#### Scenario: Unset parameters give live and offline the same defaults
- **WHEN** no overrides are stored
- **THEN** the live path and the offline path SHALL resolve the same built-in clusterer kind, merge threshold, ceiling, and gap-merge window as each other

### Requirement: Speaker-count ceiling is always enforced
Clustering SHALL never run without a speaker-count ceiling, for every clusterer kind and for every path that clusters speaker embeddings, including the clustering performed for a live recording session. When the user requests a maximum speaker count, the effective ceiling SHALL be that value; otherwise the configured default ceiling applies. An absent or non-positive user request SHALL resolve to the configured default ceiling and SHALL NOT be passed through as "no limit". The effective ceiling SHALL be clamped to the clustering backend's supported maximum (255). The pipeline SHALL NOT produce more speaker labels than the effective ceiling for a single diarization pass of one channel.

#### Scenario: Fragmented long recording is capped
- **WHEN** offline diarization runs on a long recording whose segmentation yields many short, noisy fragments and no user max-speakers is set
- **THEN** the number of distinct speaker labels in the output does not exceed the default ceiling

#### Scenario: Live session without a user maximum is capped
- **WHEN** a recording runs with online diarization and the user set no maximum speaker count
- **THEN** the session's clustering SHALL apply the configured default ceiling, and the number of distinct speaker labels produced for one channel SHALL NOT exceed it

#### Scenario: User max-speakers wins when smaller
- **WHEN** the user sets a maximum speaker count below the configured default ceiling
- **THEN** the effective ceiling equals the user's value

#### Scenario: Oversized stored ceiling is clamped
- **WHEN** a stored ceiling or user max-speakers exceeds 255
- **THEN** the effective ceiling is 255 and the clamp is logged
