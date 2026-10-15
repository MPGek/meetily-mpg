## MODIFIED Requirements

### Requirement: Voiceprint storage
The system SHALL store speaker voiceprints in a `speaker_embeddings` table where each row holds an f32 embedding blob of a fixed dimension determined by its model family (256-dimensional for the legacy polyvoice ResNet34 family, 192-dimensional for the enhanced TitaNet family), a `model` tag identifying the extractor model family, the capture `channel` ('mic' or 'system'), and the source segment duration. Each row SHALL be owned either by a registry speaker (`speaker_id`, enrolled prototype) or by a meeting cluster (`meeting_id` + `cluster_label`, unassigned cache), enforced by a CHECK constraint. Voiceprint rows SHALL be retained indefinitely.

#### Scenario: Cache written at diarization time
- **WHEN** any diarization path (offline or online) completes clustering for a meeting
- **THEN** the system SHALL persist per-cluster exemplar embeddings as unassigned cache rows and the cluster centroid with their model family, so enrollment later requires no audio re-processing

#### Scenario: Model guard
- **WHEN** the system matches voiceprints for recognition
- **THEN** it SHALL only compare embeddings whose `model` tag equals the model family used by the diarization run that produced the query embeddings

#### Scenario: Families stored together without collision
- **WHEN** a user names speakers in meetings diarized with the legacy models and later with the enhanced models
- **THEN** both families' prototypes SHALL coexist in `speaker_embeddings` under their distinct `model` tags, and recognition for any given run SHALL consider only the run's family

### Requirement: Automatic speaker recognition
After clustering completes on any diarization path, the system SHALL match each cluster against candidate prototypes (expected speakers, or all if no allowlist) using cosine similarity over embeddings of the run's model family: score = maximum similarity over the candidate's prototypes of that same family; the best-scoring candidate with score above the family's threshold (τ=0.7 for the legacy ResNet34 family, and the calibrated enhanced-family threshold for TitaNet embeddings) SHALL be auto-assigned with `matched_by='auto'` and the score recorded. When both channels' prototypes exist, same-channel prototypes SHALL be preferred.

#### Scenario: Known voice auto-labeled
- **WHEN** offline diarization completes on a meeting whose expected speakers include Alice, and a cluster centroid matches an Alice prototype with score 0.78
- **THEN** the cluster SHALL be linked to Alice with `matched_by='auto'`, and her name SHALL display on all blocks of that cluster without user action

#### Scenario: Unknown voice stays anonymous
- **WHEN** no candidate scores above the family threshold for a cluster
- **THEN** the cluster SHALL remain unidentified and display its formatted cluster label

#### Scenario: Cross-family comparisons never occur
- **WHEN** recognition runs for a meeting diarized with the enhanced models while a different speaker has only legacy-family prototypes
- **THEN** the different speaker's legacy prototypes SHALL NOT be compared against the enhanced centroids, regardless of raw similarity