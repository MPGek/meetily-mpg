## MODIFIED Requirements

### Requirement: User-assigned live labels are pinned
The live recording view SHALL treat a transcript that the user has assigned a speaker to as frozen for the rest of the session: no subsequent speaker-turn event SHALL re-match or overwrite it, regardless of which cluster the arriving turn belongs to. Only a further explicit user edit SHALL change a pinned transcript. When a user binds a cluster to a person, the live turn stream SHALL be rewritten so every turn of that cluster carries the user's chosen name with user provenance, making subsequent re-matches self-consistent.

#### Scenario: Unrelated turn does not revert a pinned label
- **WHEN** user renames cluster `SPEAKER_01` to "Alice" and a later turn for a *different* cluster (`SPEAKER_02`) arrives
- **THEN** the transcript SHALL keep displaying "Alice" and SHALL NOT be re-matched against the stale `SPEAKER_01` auto turn

#### Scenario: Old blocks stay pinned
- **WHEN** a user assigns a speaker to an old transcript block (minutes old) and new speaker turns arrive afterward
- **THEN** that block SHALL keep the user-assigned name and SHALL NOT revert to an `(auto)` name

#### Scenario: Turn stream reflects the binding
- **WHEN** a user binds cluster `SPEAKER_03` to "Alice"
- **THEN** the live turn stream SHALL present all turns of `SPEAKER_03` with display name "Alice" and user provenance for the remainder of the session

#### Scenario: Stale turn does not revert a pinned label
- **WHEN** user renames cluster `SPEAKER_01` to "Alice" and a later turn for that cluster arrives carrying a stale auto-recognized display name "Bob"
- **THEN** the transcript SHALL keep displaying "Alice" and SHALL NOT revert to "Bob"

#### Scenario: New cluster assignment still applies
- **WHEN** a later turn assigns a previously unpinned transcript to a different cluster that the user then renames
- **THEN** the transcript SHALL adopt the new cluster's label and the user's renamed display name for that cluster

### Requirement: Live speaker rename takes effect immediately (Fast mode)
During Fast-mode recording, the user SHALL be able to rename or reassign a live speaker via the same dropdown-or-text editor as offline transcripts. The editor SHALL default to single-turn scope: relabeling changes only the turn being edited (a per-turn override that persists to the matching transcript at stop), and the turn stream SHALL be rewritten so the edited turn and its matching transcript immediately display the user's name with user provenance rather than the prior auto name. With the explicit "apply to all blocks of this speaker" option, the system SHALL instead update the session's cluster-to-person binding, merge the person's prototypes into the in-memory store, and rewrite the live turn stream so all turns of that cluster carry the user's name for the remainder of the recording. Mid-recording rename in Efficient mode is NOT required (it has no live labels).

#### Scenario: Rename affects subsequent turns
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" mid-recording in Fast mode using the apply-to-all option
- **THEN** subsequent live turns recognized as that cluster SHALL display "Alice" for the rest of the session, and the binding SHALL be applied to final assignments at stop

#### Scenario: Rename affects already-emitted turns of the cluster
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" after several `SPEAKER_01` turns have already been emitted and shown with an auto name
- **THEN** those already-shown turns SHALL immediately display "Alice" with user provenance (no longer the auto name) for the remainder of the session

#### Scenario: New name creates registry person mid-session
- **WHEN** user enters a name not present in the registry during a live rename
- **THEN** the system SHALL create the registry speaker immediately and use its (initially session-derived) prototypes for matching the remainder of the session

#### Scenario: In-place live relabel
- **WHEN** user renames a live speaker (single-turn or apply-to-all) while the live transcript panel is scrolled
- **THEN** only the affected turn(s) SHALL update their displayed name in place; the panel SHALL NOT fully re-render, SHALL keep the scroll position, and SHALL NOT flash an empty/loading state

### Requirement: Per-turn speaker override during live labeling
During Fast-mode recording, the system SHALL support relabeling a single live turn to a registry speaker (existing or newly created) without changing the session's cluster binding. The per-turn override SHALL be recorded in session memory, applied to the displayed turn immediately (rewriting that turn in the live stream to the user's name with user provenance so no later re-match reverts it), and persisted at stop to the transcript matched to that turn; it SHALL NOT be overwritten by stop-time auto-recognition. When the override's time window is covered by session embeddings, those embeddings SHALL be enrolled into the assigned speaker's global prototype set as ground truth.

#### Scenario: Single misassigned turn corrected
- **WHEN** user relabels one live turn to "Bob" during a Fast-mode recording while the rest of its cluster stays linked to Alice
- **THEN** that turn SHALL display "Bob" immediately, the other turns of the cluster SHALL remain labeled as before, and at stop the transcript matched to that turn SHALL display "Bob"

#### Scenario: Single-turn override is not reverted by later turns
- **WHEN** a user relabels one live turn to "Bob" and later speaker-turn events arrive for other clusters
- **THEN** the relabeled turn SHALL keep displaying "Bob" with user provenance and SHALL NOT revert to an auto name

#### Scenario: Single-turn override enrolls ground truth
- **WHEN** user relabels one live turn to "Bob" and the turn's time window overlaps buffered session embeddings
- **THEN** the overlapping embeddings SHALL be enrolled as Bob's prototypes, in addition to the per-transcript override being persisted