## MODIFIED Requirements

### Requirement: Voiceprint storage

The system SHALL store speaker voiceprints in a `speaker_embeddings` table where each row holds a 192-dimensional f32 embedding blob from the enhanced TitaNet-Large extractor, a `model` tag identifying the extractor model (always `titanet_large` for new rows), the capture `channel` ('mic' or 'system'), and the source segment duration. Each row SHALL be owned either by a registry speaker (`speaker_id`, enrolled prototype) or by a meeting cluster (`meeting_id` + `cluster_label`, unassigned cache), enforced by a CHECK constraint. Voiceprint rows SHALL be retained indefinitely. Standard-model rows (`resnet34_int8`, 256-d) SHALL remain stored but SHALL NOT participate in recognition, enrollment seeding, or any matching operation.

When persisting exemplar embeddings for a meeting cluster, the system SHALL scope the replacement operation to the same channel: it SHALL delete only rows matching `(meeting_id, cluster_label, channel)` before inserting new rows for that channel. This ensures that cluster caches for different channels are independent and do not overwrite each other.

#### Scenario: Cache written at diarization time
- **WHEN** any diarization path (offline or online) completes clustering for a meeting using the enhanced model set
- **THEN** the system SHALL persist per-cluster exemplar embeddings as unassigned cache rows and the cluster centroid, so enrollment later requires no audio re-processing

#### Scenario: Model guard
- **WHEN** the system matches voiceprints for recognition
- **THEN** it SHALL compare only embeddings whose `model` tag equals `titanet_large`, the current extractor model; `resnet34_int8` standard-model rows SHALL be ignored entirely

#### Scenario: Channel-scoped cache replacement
- **WHEN** the system persists exemplar embeddings for a cluster with channel='system'
- **THEN** it SHALL delete only existing rows matching the same `(meeting_id, cluster_label, channel='system')` before inserting new rows, leaving any rows with channel='mic' for the same cluster label untouched

#### Scenario: Cross-channel cache independence
- **WHEN** a meeting has cluster caches for both channel='mic' and channel='system' with the same numeric pipeline speaker index (e.g., `SPEAKER_00` for mic and `SPEAKER_00` for system)
- **THEN** re-persisting one channel's cache SHALL NOT affect the other channel's cache rows
