# Spec Delta

## ADDED Requirements

### Requirement: Adjacent sub-rows of the same visible speaker render as one

The live view SHALL render adjacent sub-rows of one split block as a single sub-row when they show the same speaker. They show the same speaker when they have the same displayed name and the same ownership (user-assigned versus automatic); sub-rows without a displayed name match on cluster label. The merged sub-row SHALL span the first part's start to the last part's end and show the parts' text in order. This is display-only.

#### Scenario: Same name from different clusters merges
- **WHEN** a split block's sub-rows are clusters A, B, C in order, all displayed as "Mikhail (auto)"
- **THEN** the live view SHALL render one sub-row labelled "Mikhail (auto)" whose text is the three parts' text in order and whose window spans from A's start to C's end

#### Scenario: Different names stay separate
- **WHEN** a split block's sub-rows are displayed as "Mikhail (auto)", "Marina (auto)", "Mikhail (auto)"
- **THEN** the live view SHALL render three sub-rows, because no two adjacent sub-rows show the same speaker

#### Scenario: Unnamed different clusters stay separate
- **WHEN** two adjacent sub-rows have no displayed name and carry different cluster labels
- **THEN** they SHALL render as two sub-rows

#### Scenario: User-owned and automatic labels of the same name stay separate
- **WHEN** two adjacent sub-rows both show "Mikhail", one assigned by the user and the other automatic
- **THEN** they SHALL render as two sub-rows, the automatic one keeping its `(auto)` suffix and confirm affordance

#### Scenario: A rename that equalises neighbours merges them
- **WHEN** adjacent sub-rows show "Speaker 3" and "Mikhail (auto)" and the user renames the first to "Mikhail" and confirms the second
- **THEN** the two sub-rows SHALL render as one sub-row labelled "Mikhail"

#### Scenario: Merging does not touch persistence
- **WHEN** recording stops after merged sub-rows were shown
- **THEN** the persisted transcript rows SHALL come from the existing stop-time split path unchanged

### Requirement: A block whose runs all show one speaker renders as a single row

When merging leaves one sub-row for a split block, the live view SHALL render the block as an ordinary single row labelled with that sub-row's speaker, not as a split block surface and not with the block's segment-level label.

#### Scenario: All runs resolve to one name
- **WHEN** a block is split into sub-rows of clusters A and B and both are displayed as "Mikhail (auto)"
- **THEN** the block SHALL render as one ordinary row labelled "Mikhail (auto)"

#### Scenario: Collapsed row uses the run's label, not the segment label
- **WHEN** a block's segment-level label is "Marina (auto)" and all its live sub-rows show "Mikhail (auto)"
- **THEN** the row SHALL be labelled "Mikhail (auto)"
