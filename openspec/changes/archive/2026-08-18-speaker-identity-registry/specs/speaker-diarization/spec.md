# Spec delta: speaker-diarization

## MODIFIED Requirements

### Requirement: Speaker naming and labels
The system SHALL allow users to assign human-readable names to speaker clusters. The speaker editor SHALL default to single-block scope (a per-transcript override that relabels only the edited block); an explicit "apply to all blocks of this speaker" option SHALL link the whole cluster to a registry speaker (created on demand for new names) via the `meeting_speakers` mapping with `matched_by='user'`, and SHALL enroll the cluster's cached embeddings as that speaker's prototypes. Renaming a linked registry speaker SHALL apply globally across all meetings; display names SHALL be resolved at read time by joining `meeting_speakers` to `speakers`, with per-block overrides taking precedence and legacy `transcripts.speaker_label` as fallback.

#### Scenario: Rename speaker inline
- **WHEN** user edits a speaker label in the transcript view (e.g., changes "SPEAKER_00" to "Alice") using the default single-block scope
- **THEN** only that transcript block SHALL display "Alice"; the cluster's `meeting_speakers` mapping SHALL remain unchanged and the block's cached embeddings SHALL NOT be enrolled

#### Scenario: Name whole cluster
- **WHEN** user edits a speaker label in the transcript view with the "apply to all blocks of this speaker" option (e.g., changes "SPEAKER_00" to "Alice")
- **THEN** the system SHALL upsert the cluster's `meeting_speakers` row to link the registry speaker "Alice" (creating her if new) with `matched_by='user'`, enroll the cluster's cached embeddings to Alice, and display "Alice" on all transcripts with that cluster label

#### Scenario: Speaker label appears in UI
- **WHEN** a transcript segment's cluster is linked to a registry speaker (or has a legacy `speaker_label`)
- **THEN** the UI SHALL display the resolved name instead of the raw speaker ID

#### Scenario: Global rename
- **WHEN** user renames a linked speaker from "Alice" to "Alice Smith"
- **THEN** the name "Alice Smith" SHALL appear for that speaker in every meeting where she is linked (including any per-block overrides referencing her)

## ADDED Requirements

### Requirement: Cluster embedding cache persistence
Offline diarization SHALL persist, for each produced cluster, its centroid embedding and a bounded set of exemplar embeddings (with per-segment durations) as unassigned cache rows owned by `(meeting_id, cluster_label)`, in addition to writing transcript cluster labels.

#### Scenario: Cache survives diarization
- **WHEN** offline diarization completes for a meeting
- **THEN** each produced cluster SHALL have a stored centroid in `meeting_speakers` and exemplar embeddings in `speaker_embeddings` keyed by `(meeting_id, cluster_label)`

### Requirement: Post-clustering automatic recognition
After clustering and cache persistence, offline diarization SHALL match each cluster centroid against candidate prototypes per the speaker-identity-registry recognition rules (expected-speaker allowlist, or all registry speakers when no allowlist; model-tagged embeddings only; τ=0.7) and auto-assign confident matches.

#### Scenario: Recognized speaker labeled without user action
- **WHEN** offline diarization completes and a cluster centroid matches an expected speaker's prototype above threshold
- **THEN** the system SHALL set the cluster's `meeting_speakers.speaker_id` with `matched_by='auto'` and the match score, and the meeting's transcripts SHALL display the speaker's name

#### Scenario: No candidates leaves clusters anonymous
- **WHEN** the registry is empty or no candidate exceeds the threshold
- **THEN** diarization results SHALL be unchanged from current behavior (cluster labels only)
