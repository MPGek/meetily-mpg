## Purpose

Lets users distinguish automatically recognized speaker names from user-confirmed ones: auto-matched names render with an `(auto)` suffix and a similarity/confidence value, while names the user assigned render plain, so displayed speaker identity is trustworthy at a glance.

## ADDED Requirements

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