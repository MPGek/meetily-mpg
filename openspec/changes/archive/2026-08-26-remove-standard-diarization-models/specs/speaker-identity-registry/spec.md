# Delta spec: speaker-identity-registry

## MODIFIED Requirements

### Requirement: Voiceprint storage
The system SHALL store speaker voiceprints in a `speaker_embeddings` table where each row holds a 192-dimensional f32 embedding blob from the enhanced TitaNet-Large extractor, a `model` tag identifying the extractor model (always `titanet_large` for new rows), the capture `channel` ('mic' or 'system'), and the source segment duration. Each row SHALL be owned either by a registry speaker (`speaker_id`, enrolled prototype) or by a meeting cluster (`meeting_id` + `cluster_label`, unassigned cache), enforced by a CHECK constraint. Voiceprint rows SHALL be retained indefinitely. Standard-model rows (`resnet34_int8`, 256-d) SHALL remain stored but SHALL NOT participate in recognition, enrollment seeding, or any matching operation.

#### Scenario: Cache written at diarization time
- **WHEN** any diarization path (offline or online) completes clustering for a meeting using the enhanced model set
- **THEN** the system SHALL persist per-cluster exemplar embeddings as unassigned cache rows and the cluster centroid, so enrollment later requires no audio re-processing

#### Scenario: Model guard
- **WHEN** the system matches voiceprints for recognition
- **THEN** it SHALL compare only embeddings whose `model` tag equals `titanet_large`, the current extractor model; `resnet34_int8` standard-model rows SHALL be ignored entirely

### Requirement: Automatic speaker recognition
After clustering completes on any diarization path, the system SHALL match each cluster against candidate prototypes (expected speakers, or all if no allowlist) using cosine similarity over 192-d embeddings tagged `titanet_large`: score = maximum similarity over the candidate's prototypes; the best-scoring candidate with score above threshold τ=0.7 SHALL be auto-assigned with `matched_by='auto'` and the score recorded. When both channels' prototypes exist, same-channel prototypes SHALL be preferred. Legacy `resnet34_int8` (256-d) voiceprints and centroids SHALL NOT be loaded as match candidates.

#### Scenario: Known voice auto-labeled
- **WHEN** offline diarization completes on a meeting whose expected speakers include Alice, and a cluster centroid matches an Alice prototype with score 0.78
- **THEN** the cluster SHALL be linked to Alice with `matched_by='auto'`, and her name SHALL display on all blocks of that cluster without user action

#### Scenario: Unknown voice stays anonymous
- **WHEN** no candidate scores above threshold for a cluster
- **THEN** the cluster SHALL remain unidentified and display its formatted cluster label

#### Scenario: Standard-model voiceprints ignored
- **WHEN** recognition runs and a registry speaker's only enrolled prototypes are tagged `resnet34_int8` (256-d)
- **THEN** those prototypes SHALL be treated as absent, and the speaker SHALL NOT be auto-matched until re-enrolled with enhanced embeddings