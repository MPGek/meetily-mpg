# Spec Delta

## MODIFIED Requirements

### Requirement: Live speaker rename takes effect immediately (Fast mode)
During Fast-mode recording, the user SHALL be able to rename or reassign a live speaker via the same dropdown-or-text editor as offline transcripts. The editor SHALL default to single-turn scope: relabeling changes only the turn being edited (a per-turn override that persists to the matching transcript at stop), and the turn stream SHALL be rewritten so the edited turn and its matching transcript immediately display the user's name with user provenance rather than the prior auto name. With the explicit "apply to all blocks of this speaker" option, the system SHALL instead update the session's cluster-to-person binding, merge the person's prototypes into the in-memory store, and rewrite the live turn stream so all turns of that cluster carry the user's name for the remainder of the recording. A correction SHALL take effect on content already displayed without waiting for new speech: after the binding is recorded, the system SHALL re-evaluate the already-displayed rows of that cluster and re-emit them so their names update, rather than relying on a later speaker turn to carry the corrected name. Mid-recording rename in Efficient mode is NOT required (it has no live labels).

#### Scenario: Rename affects subsequent turns
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" mid-recording in Fast mode using the apply-to-all option
- **THEN** subsequent live turns recognized as that cluster SHALL display "Alice" for the rest of the session, and the binding SHALL be applied to final assignments at stop

#### Scenario: Rename affects already-emitted turns of the cluster
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" after several `SPEAKER_01` turns have already been emitted and shown with an auto name
- **THEN** those already-shown turns SHALL immediately display "Alice" with user provenance (no longer the auto name) for the remainder of the session

#### Scenario: Correction updates already-displayed rows without new speech
- **WHEN** user renames a live cluster and no further speech arrives on that channel
- **THEN** the rows already displayed for that cluster SHALL update to the user's name without waiting for the next speaker turn

#### Scenario: New name creates registry person mid-session
- **WHEN** user enters a name not present in the registry during a live rename
- **THEN** the system SHALL create the registry speaker immediately and use its (initially session-derived) prototypes for matching the remainder of the session

#### Scenario: In-place live relabel
- **WHEN** user renames a live speaker (single-turn or apply-to-all) while the live transcript panel is scrolled
- **THEN** only the affected turn(s) SHALL update their displayed name in place; the panel SHALL NOT fully re-render, SHALL keep the scroll position, and SHALL NOT flash an empty/loading state
