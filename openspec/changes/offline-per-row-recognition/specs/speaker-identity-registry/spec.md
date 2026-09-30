# Spec Delta

## MODIFIED Requirements

### Requirement: Per-row automatic speaker recognition
After clustering completes on any diarization path - a live session's stop-time pass or an offline pass over a saved recording - the system SHALL additionally match each transcript row of that meeting against the same candidate prototypes used for cluster recognition, using only the meeting's stored embeddings (a live session's chunk embeddings, and the exemplar embeddings an offline pass persists) whose time window overlaps that row and whose capture channel matches the row's channel, scored by the same cosine similarity over 192-d `titanet_large` embeddings and the same threshold as cluster recognition. The best-scoring candidate above threshold SHALL be recorded on the row together with its score. A row with no overlapping embedding, or whose best candidate scores below threshold, SHALL record no row-level match and SHALL keep resolving through its cluster. Before a pass records its matches, the row-level matches left by the meeting's earlier runs SHALL be cleared, so a name recorded by an earlier run cannot outrank the result of the run that just finished. Recording a row-level match SHALL NOT alter the row's stored cluster label, the cluster's centroid, the cluster's exemplar cache, or the cluster's own binding.

#### Scenario: Row keeps the speaker its own audio matches
- **WHEN** a meeting's cluster is auto-bound to Greg, and a row of that cluster has an overlapping stored embedding whose best candidate above threshold is Alex
- **THEN** that row SHALL record a row-level match to Alex with its score, while the cluster's binding to Greg SHALL remain unchanged for the rows whose own audio matches Greg

#### Scenario: Weak row evidence defers to the cluster
- **WHEN** a row's overlapping embeddings produce no candidate scoring above threshold
- **THEN** the row SHALL record no row-level match and SHALL continue to display its cluster's name

#### Scenario: Row without stored embeddings defers to the cluster
- **WHEN** a row's time window is covered by no stored embedding of the meeting
- **THEN** the row SHALL record no row-level match and SHALL continue to display its cluster's name

#### Scenario: Offline diarization is unaffected
- **WHEN** offline (batch) diarization runs on a saved recording
- **THEN** its clusters, turns, transcript speaker labels, centroids, exemplar caches and cluster bindings SHALL be exactly what they were before row-level recognition existed on that path; the only addition is the row-level matches

#### Scenario: Offline pass names a row its merged cluster would have misnamed
- **WHEN** an offline pass puts the speech of two people into one cluster whose centroid best matches Vasiliy, and one row of that cluster overlaps embeddings whose best candidate above threshold is Alex
- **THEN** that row SHALL display Alex, and the rows whose own audio matches Vasiliy SHALL display Vasiliy

#### Scenario: Offline pass names a row whose cluster centroid is below threshold
- **WHEN** an offline pass produces a cluster whose centroid scores below threshold for every candidate, and a row of that cluster overlaps an embedding scoring above threshold for Marina
- **THEN** that row SHALL display Marina, and the cluster itself SHALL stay unbound

#### Scenario: A new run replaces the earlier row-level names
- **WHEN** a meeting whose rows carry row-level matches from a live session is diarized offline
- **THEN** those earlier matches SHALL be cleared, and each row SHALL carry only what the offline run recorded, so a stale name cannot outrank the new result

#### Scenario: Offline pass preserves user decisions
- **WHEN** an offline pass runs on a meeting that has a per-block user override and a user-bound cluster
- **THEN** both SHALL survive unchanged, whatever the row-level matches say
