## ADDED Requirements

### Requirement: Suffix clears immediately on user edit
When a user assigns a speaker to a transcript block (single-block or apply-to-all, offline or live), the displayed label SHALL render as the plain name with no `(auto)` suffix and no confidence percentage immediately, without requiring a page reload or data refetch. The local view state SHALL reflect user provenance (`matched_by='user'`, cleared score) for the edited block(s).

#### Scenario: Offline edit drops the suffix in place
- **WHEN** the user assigns a speaker to a block that currently displays "Alice (auto) 78%"
- **THEN** the block SHALL immediately display "Alice" with no `(auto)` suffix or percentage, in place, without a refetch

#### Scenario: Live edit drops the suffix in place
- **WHEN** the user assigns a speaker to a live turn that currently displays "Alice (auto) 78%"
- **THEN** the turn SHALL immediately display "Alice" with no suffix, without a full re-render

#### Scenario: Apply-to-all clears every affected block
- **WHEN** the user applies a speaker to all blocks of a cluster
- **THEN** every affected block SHALL immediately lose the `(auto)` suffix in place

### Requirement: Confirming leaves no ambiguity about saved state
When the user confirms an automatically recognized speaker as correct, the system SHALL give explicit, visible feedback that the confirmation was recorded. Selecting the already-displayed name in the editor SHALL be treated as a confirmation and SHALL clear the `(auto)` suffix with user provenance, and the operation SHALL NOT silently no-op because the name did not change.

#### Scenario: Re-selecting the same name confirms
- **WHEN** a block shows "Alice (auto)" and the user picks "Alice" from the speaker editor again
- **THEN** the block SHALL be marked user-confirmed and SHALL immediately render as plain "Alice", removing the suffix

#### Scenario: Confirmed block stays plain after reload
- **WHEN** a user confirms an auto binding and then reloads the meeting
- **THEN** the block SHALL still render as plain "Alice" with user provenance

#### Scenario: Unconfirmed auto label still shows confidence
- **WHEN** a block is auto-assigned and the user has neither edited nor confirmed it
- **THEN** the block SHALL continue to display "Alice (auto) <score>%" as before
