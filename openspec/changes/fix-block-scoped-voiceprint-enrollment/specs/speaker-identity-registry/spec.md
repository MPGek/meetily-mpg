## MODIFIED Requirements

### Requirement: Speaker enrollment on assignment
When a user links a meeting cluster to a registry speaker (existing or newly created) with cluster-wide scope — including the "apply to all blocks of this speaker" option — the system SHALL enroll that cluster's cached exemplar embeddings as prototypes of the speaker by reparenting the best rows (longest duration first, capped at K=8 per enrollment). When a user assigns a speaker to a single transcript block (live or offline), the system SHALL enroll only the embeddings whose time windows overlap that block and whose capture channel matches the block's channel, capped at K=8; it SHALL NOT enroll cluster exemplars outside the block's time window. Every enrolled prototype row SHALL carry full provenance: `meeting_id`, `cluster_label`, `audio_start_time`, and `audio_end_time` of the source audio, so the prototype is playable and navigable in the Voiceprint Browser. Enrollment seeding SHALL be keyed by the underlying pipeline speaker identity and capture channel — embeddings from microphone and system channels SHALL never be mixed into the same seed set. Per-person prototype count SHALL be capped (64), pruning lowest-quality rows.

When a cluster is re-bound to a different speaker with cluster-wide scope, the system SHALL NOT leave prototypes that originated from that same cluster and channel attributed to their previous speaker: those rows SHALL be demoted to unassigned cache (following the voiceprint-rejection rules), except any row whose time window is also covered by a transcript block overridden to its current speaker.

#### Scenario: Naming enrolls voiceprint
- **WHEN** user assigns the name "Alice" to cluster `SPEAKER_00` of a diarized meeting
- **THEN** up to 8 of that cluster's cached embeddings SHALL become enrolled prototypes of speaker "Alice"

#### Scenario: Linking existing person enrolls too
- **WHEN** user links cluster `SPEAKER_01` to existing registry speaker "Bob"
- **THEN** the cluster's cached embeddings SHALL be added to Bob's prototypes, subject to the per-person cap

#### Scenario: Ground-truth block assignment enrolls its covering embeddings
- **WHEN** a user assigns "Alice" to a single transcript block whose time window overlaps several session embeddings of its cluster
- **THEN** only the embeddings whose time windows overlap that block and whose channel matches the block's channel SHALL be enrolled as prototypes of Alice, capped at K=8, at the same time as the block's identity is saved

#### Scenario: Block correction does not enroll sibling speakers
- **WHEN** a user assigns "Alice" to one block of a mixed cluster whose other blocks are correctly attributed to Bob and Carol
- **THEN** Bob's and Carol's exemplars outside Alice's block window SHALL NOT be enrolled into Alice

#### Scenario: Ground-truth enrollment preserves provenance
- **WHEN** a user assigns "Alice" to a single transcript block of meeting M, cluster C
- **THEN** the enrolled prototype rows SHALL carry `meeting_id` = M, `cluster_label` = C, and the audio timecodes of the source embeddings, so the Voiceprint Browser can display the source meeting and play the audio clip

#### Scenario: Enrollment seeding keeps channels separate
- **WHEN** a user assigns a microphone-channel block to "Alice" and a system-channel block to "Bob" in the same session
- **THEN** Alice's enrollment SHALL contain only microphone-channel embeddings and Bob's only system-channel embeddings, even when both channels share the same numeric pipeline speaker index

#### Scenario: Cluster re-binding demotes stale prototypes
- **WHEN** a cluster previously bound to "Bob" (and whose exemplars were enrolled into Bob) is re-bound cluster-wide to "Carol"
- **THEN** the prototypes that originated from that cluster and channel and are not covered by a current per-block override SHALL be demoted to unassigned cache, so Bob SHALL NOT retain them and Carol SHALL NOT inherit another speaker's exemplars

### Requirement: Per-block speaker override
The system SHALL allow relabeling a single transcript block to a registry speaker (existing or newly created) without affecting the rest of its cluster. Block-level assignment SHALL store a per-transcript override that takes precedence over the cluster mapping at display time, SHALL NOT modify `meeting_speakers`, and SHALL NOT enroll the cluster's exemplars wholesale; it SHALL enroll only the embeddings overlapping the block's time window on the block's channel, per "Speaker enrollment on assignment". Auto-recognition and re-match SHALL NOT overwrite block overrides.

#### Scenario: Single misattributed block relabeled
- **WHEN** a user assigns "Bob" to a single transcript block whose cluster maps to Alice
- **THEN** only that block SHALL display "Bob"; the cluster mapping and the other blocks' labels SHALL remain unchanged

#### Scenario: Block override survives re-match
- **WHEN** user relabels one block of a cluster and then triggers re-match or diarization runs again for the meeting
- **THEN** the relabeled block SHALL keep "Bob" while other unlinked blocks are re-matched normally

#### Scenario: Block override does not bind the cluster
- **WHEN** a user assigns "Bob" to a single transcript block with the default single-block scope
- **THEN** `meeting_speakers` SHALL NOT change for that cluster, and only embeddings overlapping that block's window SHALL be enrolled for Bob

### Requirement: Speaker editing UI with dropdown and free text
The speaker name editor (offline transcript view and live recording view) SHALL offer both a free-text input and a dropdown of registry speakers. The editor SHALL default to single-block scope: selecting an existing speaker or entering a new name links only the block being edited via a per-block override and enrolls only the embeddings overlapping that block's window on the block's channel. An explicit "apply to all blocks of this speaker" option SHALL instead link the whole cluster and enroll the cluster's cached exemplars; editing SHALL propagate to all transcript blocks sharing the same cluster label in the meeting only when that option is used.

#### Scenario: Assign from dropdown
- **WHEN** user opens the speaker editor on a transcript block and selects existing speaker "Alice" from the dropdown
- **THEN** the block SHALL be linked to Alice via a per-block override (default scope), and only the cluster exemplars overlapping the block's time window on the block's channel SHALL be enrolled; the rest of the cluster SHALL NOT be enrolled

#### Scenario: Apply to all blocks of this speaker
- **WHEN** user selects the "apply to all blocks of this speaker" option and picks "Alice"
- **THEN** the cluster SHALL be linked to Alice with `matched_by='user'`, the cluster's cached embeddings SHALL enroll to Alice, and every transcript block with that cluster label SHALL display "Alice"

#### Scenario: Propagation within meeting (explicit only)
- **WHEN** user assigns a name to one block of cluster `SPEAKER_00` using the default single-block scope
- **THEN** only that block SHALL display the resolved name; other blocks of `SPEAKER_00` SHALL be unchanged

### Requirement: Enrollment is wired into the single-block correction path
The system SHALL enroll voiceprints on single-block speaker corrections using only the embeddings whose time windows overlap that block and whose capture channel matches the block's channel, not the whole cluster and not only on cluster-wide assignment. The block's cached cluster exemplars SHALL be enrollable from a block's cluster label so a per-block correction doubles as a teaching signal, without enrolling sibling speakers' audio from a mixed cluster.

#### Scenario: Cached exemplars enrollable from a block
- **WHEN** an offline meeting has persisted cluster cache rows for a block's cluster and the user corrects that single block
- **THEN** only the cache exemplars whose time window overlaps the block and whose channel matches SHALL be enrolled for the assigned speaker, while exemplars outside the block's window SHALL remain unassigned

#### Scenario: Correction on a single-speaker cluster still enrolls
- **WHEN** the corrected block is the only speech in its cluster and the user assigns a speaker to it
- **THEN** that cluster's overlapping exemplars SHALL be enrolled as the speaker's prototypes, satisfying the "Speaker enrollment on assignment" requirement for the single-block case
