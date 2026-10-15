## MODIFIED Requirements

### Requirement: Speaker enrollment on assignment
When a user links a meeting cluster to a registry speaker (existing or newly created), the system SHALL enroll that cluster's cached exemplar embeddings as prototypes of the speaker by reparenting the best rows (longest duration first, capped at K=8 per enrollment). When a user assigns a speaker to a transcript block (live or offline, single-block or cluster-wide), the embeddings whose time windows cover that block SHALL additionally be enrolled into that speaker's global prototype set as ground truth, so the person is recognized across future meetings. Enrollment seeding SHALL be keyed by the underlying pipeline speaker identity and capture channel — embeddings from microphone and system channels SHALL never be mixed into the same seed set. Per-person prototype count SHALL be capped (64), pruning lowest-quality rows.

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

### Requirement: Speaker display name resolution
Transcript queries SHALL resolve each transcript's display name by first checking the transcript's per-block speaker override (if any), then joining its meeting's `meeting_speakers` mapping to `speakers`, and finally falling back to legacy `transcripts.speaker_label` and then to the formatted cluster label. The cluster label SHALL remain stored on the transcript row unchanged. Transcript queries SHALL also return the provenance of the resolved name (user-assigned, auto-matched, or fallback) and, for auto-matched names, the match score.

#### Scenario: Joined name displayed
- **WHEN** a transcript's cluster is linked to registry speaker Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Alice" as the display label for that transcript

#### Scenario: Block override takes precedence
- **WHEN** a transcript has a per-block speaker override to registry speaker Bob even though its cluster is linked to Alice via `meeting_speakers`
- **THEN** transcript queries SHALL return "Bob" as the display label for that transcript

#### Scenario: Legacy fallback
- **WHEN** a transcript has a legacy `speaker_label` but no `meeting_speakers` mapping and no per-block override
- **THEN** transcript queries SHALL return the legacy label

#### Scenario: Auto-matched name carries score
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='auto'` and `match_score=0.78`
- **THEN** transcript queries SHALL return the name together with 'auto' provenance and a 0.78 score

#### Scenario: User-assigned name carries user provenance
- **WHEN** a transcript's display name resolves through a `meeting_speakers` row with `matched_by='user'`
- **THEN** transcript queries SHALL return the name together with 'user' provenance and no score