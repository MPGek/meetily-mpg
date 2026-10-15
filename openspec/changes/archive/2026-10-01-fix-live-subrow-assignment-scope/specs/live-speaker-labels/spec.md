# Spec Delta

## MODIFIED Requirements

### Requirement: Per-turn speaker override during live labeling
During Fast-mode recording, the system SHALL support relabeling a single live turn to a registry speaker (existing or newly created) without changing the session's cluster binding. The per-turn override SHALL be recorded in session memory, applied to the displayed turn immediately, and persisted at stop to the transcript matched to that turn; it SHALL NOT be overwritten by stop-time auto-recognition. When the override's time window is covered by session embeddings, those embeddings SHALL be enrolled into the assigned speaker's global prototype set as ground truth. A session embedding SHALL be enrolled for a given speaker at most once per stop, however many of that speaker's overrides overlap it, and repeating an edit or confirmation on the same turn SHALL NOT enroll additional copies.

#### Scenario: Single misassigned turn corrected
- **WHEN** user relabels one live turn to "Bob" during a Fast-mode recording while the rest of its cluster stays linked to Alice
- **THEN** that turn SHALL display "Bob" immediately, the other turns of the cluster SHALL remain labeled as before, and at stop the transcript matched to that turn SHALL display "Bob"

#### Scenario: Single-turn override enrolls ground truth
- **WHEN** user relabels one live turn to "Bob" and the turn's time window overlaps buffered session embeddings
- **THEN** the overlapping embeddings SHALL be enrolled as Bob's prototypes, in addition to the per-transcript override being persisted

#### Scenario: Several overrides over one embedding enroll it once
- **WHEN** during one recording the user assigns "Bob" to three sub-rows whose windows all overlap the same buffered session embedding, and then stops
- **THEN** Bob SHALL gain exactly one prototype for that embedding, not three

#### Scenario: Repeated confirmation does not duplicate
- **WHEN** the user confirms the same live turn as "Bob" twice before stopping
- **THEN** the embeddings overlapping that turn SHALL each be enrolled for Bob once

### Requirement: Per-turn overrides and pinned labels survive live splits

When a live transcript block is split into sub-rows by live word-level diarization, existing live user assignments for that block SHALL carry across the revision: a single-turn override SHALL apply to the sub-row whose displayed window covers the override's time window on the same channel, and a label pinned on the whole block before it was split SHALL apply to every sub-row of the same cluster. An override SHALL keep applying to its window when a later revision attributes that window to a different cluster label. When several overrides cover the same sub-row, the most recently made one SHALL apply.

An edit or confirmation made on one sub-row of an already-split block SHALL apply to that sub-row only: it SHALL NOT change the displayed name, provenance, or confirm affordance of any other sub-row, and SHALL NOT replace or clear a label pinned on the block. A sub-row edit SHALL still exempt its parent transcript from later automatic re-matching. Only user edits clear pinning.

#### Scenario: Split does not revert a pinned label
- **WHEN** a user pinned a transcript block's label to "Alice" and the turn stream later triggers a live revision that splits the block
- **THEN** every sub-row carrying the pinned cluster label SHALL continue displaying "Alice" and SHALL NOT re-render under the auto/recognized label

#### Scenario: Single-turn override lands on the covering sub-row
- **WHEN** a single-turn override covering time [t1, t2] exists and the block it was applied to is later live-split into sub-rows
- **THEN** the sub-row whose time window overlaps [t1, t2] SHALL display the override's name immediately, and the other sub-rows SHALL NOT receive it because the override is window-scoped to that sub-row

#### Scenario: Editing one sub-row leaves same-cluster siblings alone
- **WHEN** a split block shows sub-rows of clusters A, B, A, all auto-labeled "Alice (auto)", and the user assigns "Bob" to the first sub-row
- **THEN** only the first sub-row SHALL display "Bob"; the third sub-row, although of cluster A, SHALL keep displaying "Alice (auto)" with its confirm affordance

#### Scenario: A later sub-row edit does not revert an earlier one
- **WHEN** the user assigns "Bob" to sub-row 2 of a split block and then confirms sub-row 3, which belongs to a different cluster
- **THEN** sub-row 2 SHALL keep displaying "Bob" with user provenance, and sub-row 3 SHALL display its confirmed name

#### Scenario: Confirm affordance changes only on the confirmed sub-row
- **WHEN** the user confirms one auto-labeled sub-row of a split block
- **THEN** that sub-row SHALL lose its `(auto)` suffix and confirm affordance, and no other sub-row SHALL gain or lose either

#### Scenario: Re-attribution to another cluster keeps the override
- **WHEN** the user assigned "Bob" to a sub-row of cluster A and a later live revision attributes the same time window to cluster C
- **THEN** the sub-row covering that window SHALL keep displaying "Bob" with user provenance

#### Scenario: Latest override wins
- **WHEN** the user assigns "Bob" to a sub-row and later assigns "Carol" to the same sub-row
- **THEN** the sub-row SHALL display "Carol"

#### Scenario: Pin from before the split still applies after a sub-row edit
- **WHEN** the user pinned an unsplit block to "Alice", the block was then split into sub-rows of clusters A, B, A, and the user assigns "Bob" to the second sub-row
- **THEN** the second sub-row SHALL display "Bob" and both cluster-A sub-rows SHALL keep displaying "Alice"
