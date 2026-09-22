# Spec Delta

## MODIFIED Requirements

### Requirement: Automatic labels marked with provenance
The system SHALL expose whether a displayed speaker identity was assigned by automatic recognition — whether that name came from the transcript's own row-level match or from its cluster's binding (`meeting_speakers.matched_by = 'auto'`) — or by the user (`matched_by = 'user'`, or a per-block override), together with the recognition score of the level that resolved the name, to every surface that renders a speaker name (meeting details transcript queries and the live recording view).

#### Scenario: Auto-matched cluster carries score
- **WHEN** a transcript's cluster is linked to a registry speaker with `matched_by='auto'` and `match_score` recorded
- **THEN** transcript queries SHALL return the speaker name, a provenance flag indicating automatic matching, and the match score

#### Scenario: Auto-matched row carries its own score
- **WHEN** a transcript's displayed name comes from its row-level automatic match with a recorded score
- **THEN** transcript queries SHALL return that name, a provenance flag indicating automatic matching, and that row's score rather than the cluster's

#### Scenario: User-assigned cluster marked as user
- **WHEN** a transcript's cluster is linked to a registry speaker with `matched_by='user'`
- **THEN** transcript queries SHALL return the speaker name with provenance indicating a user assignment
