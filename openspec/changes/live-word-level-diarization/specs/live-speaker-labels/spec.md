# live-speaker-labels Delta (part of live-word-level-diarization)

## ADDED Requirements

### Requirement: Per-turn overrides and pinned labels survive live splits

When a live transcript block is split into sub-rows by live word-level diarization, existing live user assignments for that block SHALL carry across the revision: a single-turn override SHALL apply to the sub-row whose displayed window covers the override's time window, and a pinned cluster-level label SHALL apply to every sub-row of the same cluster. A further explicit user edit SHALL still be able to change or pin a sub-row independently; only user edits clear pinning.

#### Scenario: Split does not revert a pinned label
- **WHEN** a user pinned a transcript block's label to "Alice" and the turn stream later triggers a live revision that splits the block
- **THEN** every sub-row carrying the pinned cluster label SHALL continue displaying "Alice" and SHALL NOT re-render under the auto/recognized label

#### Scenario: Single-turn override lands on the covering sub-row
- **WHEN** a single-turn override covering time [t1, t2] exists and the block it was applied to is later live-split into sub-rows
- **THEN** the sub-row whose time window overlaps [t1, t2] SHALL display the override's name immediately, and the other sub-rows SHALL NOT receive it because the override is window-scoped to that sub-row
