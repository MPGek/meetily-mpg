# speaker-label-confidence Specification

## Purpose
Lets users distinguish automatically recognized speaker names from user-confirmed ones: auto-matched names render with an `(auto)` suffix and a similarity/confidence value, while names the user assigned render plain, so displayed speaker identity is trustworthy at a glance.

## Requirements

### Requirement: Automatic labels marked with provenance
The system SHALL expose whether a displayed speaker identity was assigned by automatic recognition (`meeting_speakers.matched_by = 'auto'`) or by the user (`matched_by = 'user'`), together with the recognition score, to every surface that renders a speaker name (meeting details transcript queries and the live recording view).

#### Scenario: Auto-matched cluster carries score
- **WHEN** a transcript's cluster is linked to a registry speaker with `matched_by='auto'` and `match_score` recorded
- **THEN** transcript queries SHALL return the speaker name, a provenance flag indicating automatic matching, and the match score

#### Scenario: User-assigned cluster marked as user
- **WHEN** a transcript's cluster is linked to a registry speaker with `matched_by='user'`
- **THEN** transcript queries SHALL return the speaker name with provenance indicating a user assignment

### Requirement: Display formatting of speaker labels
The speaker label renderer SHALL append an `(auto)` suffix and the similarity score (as a percentage) to names whose identity was automatically matched, and SHALL render the plain name with no suffix for user-assigned identities. Legacy fallbacks (no `meeting_speakers` mapping) SHALL keep current behavior.

#### Scenario: Auto name with suffix and confidence
- **WHEN** a transcript's display name resolves to "Alice" via an auto match with score 0.78
- **THEN** the label renders as "Alice (auto)" with a confidence indication (e.g., "78%")

#### Scenario: User name without suffix
- **WHEN** a transcript's display name resolves to "Alice" via a user binding
- **THEN** the label renders as "Alice" with no suffix

#### Scenario: Unlinked cluster falls back unchanged
- **WHEN** a transcript's cluster has no `meeting_speakers` mapping
- **THEN** the label renders the formatted cluster label exactly as today, with no provenance decoration

### Requirement: Live speaker-turn events carry provenance and confidence
The `online-speaker-turn` event emitted during Fast-mode recording SHALL carry, for each recognized turn, the recognized display name, whether the recognition was automatic or user-bound, and the match score, so the live view can apply the same suffix/confidence formatting as the saved view.

#### Scenario: Recognized live turn carries score
- **WHEN** a live turn matches an expected speaker above threshold during Fast-mode recording
- **THEN** the emitted turn SHALL include the display name, provenance (auto/user), and the match score

#### Scenario: Unknown live turn has no provenance
- **WHEN** a live turn matches no registry speaker above threshold
- **THEN** the emitted turn SHALL carry no recognition provenance and SHALL display the formatted cluster label

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
