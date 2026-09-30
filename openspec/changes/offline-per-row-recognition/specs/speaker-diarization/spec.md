# Spec Delta

## MODIFIED Requirements

### Requirement: Post-clustering automatic recognition
After clustering and cache persistence, offline diarization SHALL match each cluster centroid against candidate prototypes per the speaker-identity-registry recognition rules (expected-speaker allowlist, or all registry speakers when no allowlist; model-tagged embeddings only; τ=0.7) and auto-assign confident matches. It SHALL then additionally name each transcript row from the embeddings that overlap that row, per the speaker-identity-registry per-row recognition rules, so that a cluster holding the speech of more than one person, or whose centroid falls below the threshold, does not decide the name of a row whose own audio matches a candidate. A failure of the row-level step SHALL NOT fail the diarization or change its reported status; the rows then resolve through their cluster bindings as they did before the step existed.

#### Scenario: Recognized speaker labeled without user action
- **WHEN** offline diarization completes and a cluster centroid matches an expected speaker's prototype above threshold
- **THEN** the system SHALL set the cluster's `meeting_speakers.speaker_id` with `matched_by='auto'` and the match score, and the meeting's transcripts SHALL display the speaker's name

#### Scenario: No candidates leaves clusters anonymous
- **WHEN** the registry is empty or no candidate exceeds the threshold
- **THEN** diarization results SHALL be unchanged from current behavior (cluster labels only)

#### Scenario: A merged cluster does not rename a row whose own audio matches someone else
- **WHEN** offline diarization puts two recognizable people into one cluster and the cluster's centroid matches only one of them
- **THEN** the rows of the other person SHALL display that person's name, not the cluster's

#### Scenario: Row-level failure does not fail the diarization
- **WHEN** the row-level step fails after the clusters have been persisted and recognized
- **THEN** the diarization SHALL still complete with its normal status and result, the failure SHALL be logged, and the rows SHALL resolve through their cluster bindings

#### Scenario: The diarization output itself is unchanged
- **WHEN** offline diarization records row-level matches
- **THEN** its clusters, turns, transcript speaker labels, centroids, exemplar caches and cluster bindings SHALL be exactly what they would be without the row-level step
