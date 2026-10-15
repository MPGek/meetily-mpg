# Spec Delta

## MODIFIED Requirements

### Requirement: Per-turn overrides and pinned labels survive live splits

When a live transcript block is split into sub-rows by live word-level diarization, existing live user assignments for that block SHALL carry across the revision: a single-turn override SHALL apply to the sub-row whose displayed window covers the override's time window on the same channel, and a label pinned on the whole block before it was split SHALL apply to every sub-row of the same cluster. An override SHALL keep applying to its window when a later revision attributes that window to a different cluster label. When several overrides cover the same sub-row, the most recently made one SHALL apply.

An edit or confirmation made on one sub-row of an already-split block SHALL apply to that sub-row only: it SHALL NOT change the displayed name, provenance, or confirm affordance of any other sub-row, and SHALL NOT replace or clear a label pinned on the block. A sub-row merged from adjacent sub-rows of the same visible speaker counts as one sub-row: an edit on it applies to all of its parts. A sub-row edit SHALL still exempt its parent transcript from later automatic re-matching. Only user edits clear pinning.

#### Scenario: Split does not revert a pinned label
- **WHEN** a user pinned a transcript block's label to "Alice" and the turn stream later triggers a live revision that splits the block
- **THEN** every sub-row carrying the pinned cluster label SHALL continue displaying "Alice" and SHALL NOT re-render under the auto/recognized label

#### Scenario: Single-turn override lands on the covering sub-row
- **WHEN** a single-turn override covering time [t1, t2] exists and the block it was applied to is later live-split into sub-rows
- **THEN** the sub-row whose time window overlaps [t1, t2] SHALL display the override's name immediately, and the other sub-rows SHALL NOT receive it because the override is window-scoped to that sub-row

#### Scenario: Editing one sub-row leaves same-cluster siblings alone
- **WHEN** a split block shows sub-rows of clusters A, B, A, labelled "Alice (auto)", "Carol (auto)", "Alice (auto)", and the user assigns "Bob" to the first sub-row
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
- **WHEN** the user pinned an unsplit block to "Alice", the block was then split into sub-rows of clusters A, B, A with B displayed as "Carol (auto)", and the user assigns "Bob" to the second sub-row
- **THEN** the second sub-row SHALL display "Bob" and both cluster-A sub-rows SHALL keep displaying "Alice"

## ADDED Requirements

### Requirement: Edits on a merged sub-row apply to every part

An assignment, confirmation, or apply-to-all rename made on a merged sub-row, or on a block collapsed into one row, SHALL be recorded for every constituent cluster. A single-row edit SHALL be scoped to each part's own time window. The whole row SHALL update immediately, and at stop the user's choice SHALL persist to every stored row the merged sub-row covered.

#### Scenario: Single-row rename of a merged sub-row
- **WHEN** a merged sub-row is made of cluster A over [10 s, 14 s] and cluster C over [14 s, 20 s], and the user assigns "Bob" to it without the apply-to-all option
- **THEN** the whole sub-row SHALL display "Bob" immediately, and after stop the stored rows of cluster A overlapping [10 s, 14 s] and of cluster C overlapping [14 s, 20 s] SHALL show "Bob"

#### Scenario: Apply-to-all rename binds every constituent cluster
- **WHEN** the user renames a merged sub-row made of clusters A and C to "Bob" with the apply-to-all option
- **THEN** every displayed row and sub-row of cluster A or cluster C SHALL display "Bob" for the rest of the session

#### Scenario: Confirming a merged sub-row confirms all parts
- **WHEN** the user confirms a merged sub-row labelled "Mikhail (auto)" made of clusters A and C
- **THEN** the sub-row SHALL lose its `(auto)` suffix and confirm affordance, and after stop the stored rows it covered SHALL show "Mikhail" with user provenance

#### Scenario: Collapsed row edits behave like a merged sub-row
- **WHEN** a block whose sub-rows all show "Mikhail (auto)" renders as one ordinary row, and the user assigns "Bob" to it
- **THEN** the row SHALL display "Bob" and the assignment SHALL be recorded for each constituent cluster over its own window
