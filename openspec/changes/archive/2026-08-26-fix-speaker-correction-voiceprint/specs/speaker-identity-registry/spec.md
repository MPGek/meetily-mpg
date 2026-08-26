## MODIFIED Requirements

### Requirement: Speaker enrollment on assignment
When a user links a meeting cluster to a registry speaker (existing or newly created), the system SHALL enroll that cluster's cached exemplar embeddings as prototypes of the speaker by reparenting the best rows (longest duration first, capped at K=8 per enrollment). When a user assigns a speaker to a transcript block (live or offline, single-block or cluster-wide), the embeddings whose time windows cover that block SHALL additionally be enrolled into that speaker's global prototype set as ground truth, so the person is recognized across future meetings. Enrollment seeding SHALL be keyed by the underlying pipeline speaker identity and capture channel — embeddings from microphone and system channels SHALL never be mixed into the same seed set. Per-person prototype count SHALL be capped (64), pruning lowest-quality rows. When a transcript block has no cluster label (speaker column is NULL), the system SHALL resolve the cluster by matching the block's time range against stored cluster centroids for the meeting and channel before enrolling.

#### Scenario: Naming enrolls voiceprint
- **WHEN** user assigns the name "Alice" to cluster `SPEAKER_00` of a diarized meeting
- **THEN** up to 8 of that cluster's cached embeddings SHALL become enrolled prototypes of speaker "Alice"

#### Scenario: Linking existing person enrolls too
- **WHEN** user links cluster `SPEAKER_01` to existing registry speaker "Bob"
- **THEN** the cluster's cached embeddings SHALL be added to Bob's prototypes, subject to the per-person cap

#### Scenario: Ground-truth block assignment enrolls its covering embeddings
- **WHEN** a user assigns "Alice" to a single transcript block whose time window is covered by session embeddings
- **THEN** the embeddings overlapping that block's time window SHALL be enrolled as prototypes of Alice at the same time as the block's identity is saved

#### Scenario: Enrollment seeding keeps channels separate
- **WHEN** a user assigns a microphone-channel block to "Alice" and a system-channel block to "Bob" in the same session
- **THEN** Alice's enrollment SHALL contain only microphone-channel embeddings and Bob's only system-channel embeddings, even when both channels share the same numeric pipeline speaker index

#### Scenario: Block correction with NULL cluster resolves by time overlap
- **WHEN** a user assigns "Alice" to a single transcript block whose `speaker` column is NULL but whose time range overlaps a stored cluster centroid
- **THEN** the system SHALL resolve the cluster by time-overlap matching, enroll the resolved cluster's cached exemplars as Alice's prototypes, and set the per-block override
