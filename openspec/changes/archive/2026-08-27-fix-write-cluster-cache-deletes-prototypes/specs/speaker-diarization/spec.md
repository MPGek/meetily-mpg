## MODIFIED Requirements

### Requirement: Cluster embedding cache persistence
Offline diarization SHALL persist, for each produced cluster, its centroid embedding and a bounded set of exemplar embeddings (with per-segment durations) as unassigned cache rows owned by `(meeting_id, cluster_label)`, in addition to writing transcript cluster labels. When refreshing a cluster's exemplar cache, the system SHALL only replace unassigned cache rows (where `speaker_id IS NULL`) and SHALL NOT delete enrolled prototypes (where `speaker_id IS NOT NULL`).

#### Scenario: Cache survives diarization
- **WHEN** offline diarization completes for a meeting
- **THEN** each produced cluster SHALL have a stored centroid in `meeting_speakers` and exemplar embeddings in `speaker_embeddings` keyed by `(meeting_id, cluster_label)`

#### Scenario: Cache refresh preserves enrolled prototypes
- **WHEN** offline diarization re-runs on a meeting that already has enrolled prototypes for one or more clusters
- **THEN** the cache refresh SHALL replace only the unassigned exemplar rows for each cluster and SHALL NOT delete any enrolled prototypes, so that voiceprints enrolled during the recording session or via manual assignment survive re-diarization
