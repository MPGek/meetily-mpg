## ADDED Requirements

### Requirement: Confirm an auto-assigned binding as correct
The system SHALL let the user confirm that an automatically recognized speaker binding is correct without changing the name. Confirming SHALL flip the binding to user provenance: for a cluster-binding share, `meeting_speakers.matched_by` SHALL become `'user'` and `match_score` SHALL be cleared; for a single-block override, the block SHALL be marked overridden so it reads as user provenance. Confirming SHALL NOT re-enroll or duplicate the already-present voiceprint.

#### Scenario: Confirm a recognized cluster binding
- **WHEN** a cluster is auto-bound to "Alice" with a match score and the user confirms it is correct
- **THEN** the cluster's `matched_by` SHALL become `'user'` and its match score SHALL be cleared, so the `(auto)` decoration no longer renders

#### Scenario: Confirm a recognized single block
- **WHEN** a single transcript block is auto-bound to "Alice" with user confirmation
- **THEN** the block SHALL be marked as user-confirmed and SHALL render with plain "Alice", losing the `(auto)` decoration

#### Scenario: Confirmation does not duplicate prototypes
- **WHEN** the user confirms an auto-assigned binding whose speaker already has enrolled prototypes
- **THEN** no new `speaker_embeddings` rows SHALL be created for that confirmation

### Requirement: Enrollment is wired into the single-block correction path
The system SHALL enroll voiceprints on single-block speaker corrections using the embeddings covering that block, not only on cluster-wide assignment. Existing cached cluster exemplars SHALL be enrollable from a block's cluster label so a per-block correction doubles as a teaching signal.

#### Scenario: Cached exemplars enrollable from a block
- **WHEN** an offline meeting has persisted cluster cache rows for a block's cluster and the user corrects that single block
- **THEN** the block's correction SHALL enroll the cluster's exemplar embeddings for the assigned speaker, satisfying the existing "Speaker enrollment on assignment" requirement for the single-block case

### Requirement: Voiceprint browser storage summary is human-readable
The voiceprint browser's storage summary SHALL display total voiceprint storage as a human-readable size (e.g. megabytes) using the same formatting as the Settings general-tab storage section, rather than a raw byte count. The speaker, prototype, and unconfirmed-cache counts SHALL remain numeric.

#### Scenario: Size shown in megabytes
- **WHEN** the voiceprint browser loads with 2,258,944 total embedding bytes
- **THEN** the summary SHALL show a human-readable size (e.g. "2.2 MB") rather than "Bytes: 2258944"

#### Scenario: Empty storage shows zero
- **WHEN** the voiceprint browser loads with zero total embedding bytes
- **THEN** the summary SHALL show "0 MB" rather than "Bytes: 0"

#### Scenario: Consistent with the general-tab storage section
- **WHEN** the voiceprint browser storage summary and the Settings general-tab storage section both render
- **THEN** both SHALL format embedding size the same way, so the two surfaces agree

### Requirement: Voiceprint browser groups start collapsed
The voiceprint browser SHALL load with every speaker group and every unconfirmed-meeting group collapsed by default, showing group headers and the expand controls first rather than every voiceprint row expanded. The existing per-group toggles and expand-all / collapse-all controls SHALL continue to work.

#### Scenario: Default state is collapsed
- **WHEN** the voiceprint browser first loads with voiceprints present
- **THEN** every speaker group and meeting group SHALL start collapsed with their rows hidden

#### Scenario: Expand-all reveals every group
- **WHEN** the user activates expand-all
- **THEN** every speaker and meeting group SHALL become expanded

#### Scenario: Single group expands independently
- **WHEN** the user expands one collapsed group
- **THEN** only that group's rows SHALL be shown while the others remain collapsed
