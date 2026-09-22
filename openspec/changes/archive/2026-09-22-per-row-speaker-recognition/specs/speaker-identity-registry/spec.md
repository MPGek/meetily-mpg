# Spec Delta

## ADDED Requirements

### Requirement: Per-row automatic speaker recognition
After a live session's clustering completes, the system SHALL additionally match each transcript row of that meeting against the same candidate prototypes used for cluster recognition, using only the session's stored embeddings whose time window overlaps that row and whose capture channel matches the row's channel, scored by the same cosine similarity over 192-d `titanet_large` embeddings and the same threshold as cluster recognition. The best-scoring candidate above threshold SHALL be recorded on the row together with its score. A row with no overlapping embedding, or whose best candidate scores below threshold, SHALL record no row-level match and SHALL keep resolving through its cluster. Recording a row-level match SHALL NOT alter the row's stored cluster label, the cluster's centroid, the cluster's exemplar cache, or the cluster's own binding.

#### Scenario: Row keeps the speaker its own audio matches
- **WHEN** a meeting's cluster is auto-bound to Greg, and a row of that cluster has an overlapping stored embedding whose best candidate above threshold is Alex
- **THEN** that row SHALL record a row-level match to Alex with its score, while the cluster's binding to Greg SHALL remain unchanged for the rows whose own audio matches Greg

#### Scenario: Weak row evidence defers to the cluster
- **WHEN** a row's overlapping embeddings produce no candidate scoring above threshold
- **THEN** the row SHALL record no row-level match and SHALL continue to display its cluster's name

#### Scenario: Row without stored embeddings defers to the cluster
- **WHEN** a row's time window is covered by no stored embedding of the session
- **THEN** the row SHALL record no row-level match and SHALL continue to display its cluster's name

#### Scenario: Offline diarization is unaffected
- **WHEN** offline (batch) diarization runs on a saved recording
- **THEN** recognition SHALL behave exactly as before this change, and no row-level match SHALL be recorded by that path

## MODIFIED Requirements

### Requirement: Speaker display name resolution
Transcript queries SHALL resolve each transcript's display name by first checking the transcript's per-block speaker override (if any), then the transcript's own row-level automatic match (if any), then joining its meeting's `meeting_speakers` mapping to `speakers`, and finally falling back to legacy `transcripts.speaker_label` and then to the formatted cluster label. A row-level automatic match SHALL NOT take precedence over a per-block user override, nor over a cluster binding the user made (`matched_by='user'`). The cluster label SHALL remain stored on the transcript row unchanged. Transcript queries SHALL also return the provenance of the resolved name (user-assigned, auto-matched, or fallback) and, for auto-matched names, the match score of the level that resolved the name.

#### Scenario: Joined name displayed
- **WHEN** a transcript's cluster is linked to registry speaker Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Alice" as the display label for that transcript

#### Scenario: Block override takes precedence
- **WHEN** a transcript has a per-block speaker override to registry speaker Bob even though its cluster is linked to Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Bob" as the display label for that transcript

#### Scenario: Row-level match takes precedence over the cluster binding
- **WHEN** a transcript has a row-level automatic match to Alex with score 0.74 and its cluster is auto-linked to Greg
- **THEN** transcript queries SHALL return "Alex" with auto provenance and the 0.74 score

#### Scenario: A user-bound cluster outranks a row-level match
- **WHEN** a transcript's cluster is linked to Alice with `matched_by='user'` and the transcript also carries a row-level automatic match to Alex
- **THEN** transcript queries SHALL return "Alice" with user provenance

#### Scenario: Row without a match resolves through its cluster
- **WHEN** a transcript carries no row-level automatic match and its cluster is auto-linked to Greg with score 0.86
- **THEN** transcript queries SHALL return "Greg" with auto provenance and the 0.86 score

#### Scenario: Legacy fallback
- **WHEN** a transcript has a legacy `speaker_label` but no `meeting_speakers` mapping, no row-level match and no per-block override
- **THEN** transcript queries SHALL return the legacy label

#### Scenario: Auto-matched name carries score
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='auto'` and `match_score=0.78`
- **THEN** transcript queries SHALL return the name together with 'auto' provenance and a 0.78 score

#### Scenario: User-assigned name carries user provenance
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='user'`
- **THEN** transcript queries SHALL return the name together with 'user' provenance and no score

### Requirement: Instant re-match on allowlist change
Because cluster centroids are cached in `meeting_speakers` and per-cluster exemplar embeddings are cached with their time windows, the system SHALL provide a re-match operation that re-runs recognition for a meeting from cached data only, without reading or processing audio. Re-match SHALL refresh the row-level automatic matches of that meeting from the cached embeddings under the current candidate set, and SHALL clear a row-level match that the current candidate set no longer supports, so a refreshed cluster binding can never be outranked by a stale row-level name. User bindings (`matched_by='user'`) and per-block user overrides SHALL be preserved.

#### Scenario: Allowlist edit resolves pending clusters
- **WHEN** user adds Dave to a meeting's expected speakers after diarization and triggers re-match
- **THEN** previously unidentified clusters SHALL be matched against Dave's prototypes and auto-assigned where confident, with no diarization re-run

#### Scenario: Allowlist edit refreshes row-level names
- **WHEN** a meeting has row-level matches to Greg, the user removes Greg from the expected speakers and triggers re-match
- **THEN** those row-level matches SHALL be recomputed against the remaining candidates, and any that no longer score above threshold SHALL be cleared so the rows resolve through their cluster again

#### Scenario: Re-match preserves user decisions
- **WHEN** a meeting has a per-block override and a user-bound cluster, and the user triggers re-match
- **THEN** both SHALL survive re-match unchanged, regardless of what the refreshed row-level matches say
