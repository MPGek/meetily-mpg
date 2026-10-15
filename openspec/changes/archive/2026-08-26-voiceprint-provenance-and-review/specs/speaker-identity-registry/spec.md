## MODIFIED Requirements

### Requirement: Voiceprint storage
The system SHALL store speaker voiceprints in a `speaker_embeddings` table where each row holds a 256-dimensional f32 embedding blob, a `model` tag identifying the extractor model, the capture `channel` ('mic' or 'system'), the source segment duration, and its provenance: the `meeting_id`, `cluster_label`, and `audio_start_time`/`audio_end_time` of the source segment, when known. Each row SHALL be owned by a registry speaker (`speaker_id` set: enrolled prototype) or be unassigned (`speaker_id` NULL with `meeting_id` + `cluster_label` set: unassigned cache). An enrolled prototype SHALL retain its provenance — `meeting_id`, `cluster_label`, and timecodes — rather than having it cleared. Rows without known provenance SHALL be representable with provenance unset and the system SHALL NOT fabricate provenance. Voiceprint rows SHALL be retained indefinitely. Deleting a meeting SHALL delete its unassigned cache rows and SHALL NOT delete enrolled prototypes that merely reference the meeting in their provenance.

#### Scenario: Cache written at diarization time
- **WHEN** any diarization path (offline or online) completes clustering for a meeting
- **THEN** the system SHALL persist per-cluster exemplar embeddings as unassigned cache rows and the cluster centroid, with the source segment's `audio_start_time`/`audio_end_time` and `meeting_id`/`cluster_label`, so enrollment later requires no audio re-processing

#### Scenario: Prototype retains its provenance
- **WHEN** a user links cluster `SPEAKER_01` of meeting M2 (whose caches carry timecodes 40.0–43.2) to registry speaker "Bob" and its caches enroll to him
- **THEN** Bob's enrolled prototypes SHALL retain `meeting_id`=M2, `cluster_label`=`SPEAKER_01`, `audio_start_time`=40.0, and `audio_end_time`=43.2

#### Scenario: Meeting deletion keeps provenanced prototypes
- **WHEN** a meeting is deleted and an enrolled prototype references that meeting only as provenance
- **THEN** the prototype SHALL remain part of its speaker's voiceprint set; only the meeting's unassigned cache rows and mappings SHALL be deleted

#### Scenario: Legacy rows lack provenance
- **WHEN** a voiceprint row predates provenance tracking
- **THEN** its provenance fields SHALL be unset, the row SHALL remain usable for recognition, and no source meeting or time range SHALL be fabricated

#### Scenario: Model guard
- **WHEN** the system matches voiceprints for recognition
- **THEN** it SHALL only compare embeddings whose `model` tag equals the currently configured extractor model

### Requirement: Speaker enrollment on assignment
When a user links a meeting cluster to a registry speaker (existing or newly created), the system SHALL enroll that cluster's cached exemplar embeddings as prototypes of the speaker by reparenting the best rows (longest duration first, capped at K=8 per enrollment). When a user assigns a speaker to a transcript block (live or offline, single-block or cluster-wide), the embeddings whose time windows cover that block SHALL additionally be enrolled into that speaker's global prototype set as ground truth, so the person is recognized across future meetings. Enrollment seeding SHALL be keyed by the underlying pipeline speaker identity and capture channel — embeddings from microphone and system channels SHALL never be mixed into the same seed set. Enrollment SHALL preserve each row's provenance (`meeting_id`, `cluster_label`, `audio_start_time`, `audio_end_time`); only the owning `speaker_id` SHALL change. Per-person prototype count SHALL be capped (64), pruning lowest-quality rows.

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

#### Scenario: Provenance survives enrollment reparenting
- **WHEN** a cluster's cache rows are enrolled to a speaker
- **THEN** the reparented prototype rows SHALL keep their original `meeting_id`, `cluster_label`, and timecodes, so the review UI can still locate and play their source audio
