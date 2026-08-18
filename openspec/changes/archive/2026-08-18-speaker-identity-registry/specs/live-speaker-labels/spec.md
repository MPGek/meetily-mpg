# Spec delta: live-speaker-labels

## MODIFIED Requirements

### Requirement: Live labels are display-only until stop

Live speaker assignment SHALL update only in-memory frontend state during recording; persistence of transcript speaker assignments SHALL still occur at recording stop through the existing `recording-stopped`/`speaker_assignments` path. Live rename operations MAY create registry speaker rows and update in-memory session state during recording, but SHALL NOT write transcript speaker assignments before stop.

#### Scenario: No persistence during recording
- **WHEN** speaker labels are shown live during recording
- **THEN** no database write SHALL occur for transcript speaker assignments

#### Scenario: Stop-time assignments remain authoritative
- **WHEN** recording stops
- **THEN** the system SHALL compute and persist final speaker assignments via the existing stop-time path, reconciled with the session's cluster-to-person bindings (including live renames)

## ADDED Requirements

### Requirement: Live labels reflect recognized speakers
During Fast-mode recording, the system SHALL match each chunk embedding against an in-memory prototype store (loaded at recording start from the meeting's expected speakers, or all registry speakers when no list was provided) and SHALL display the recognized registry speaker's name on matching live turns instead of the raw cluster label.

#### Scenario: Known voice named live
- **WHEN** a Fast-mode recording has expected speaker Alice and an incoming chunk embedding matches Alice's prototypes above threshold
- **THEN** the live turn SHALL display "Alice"

#### Scenario: Unknown voice shows cluster label
- **WHEN** no prototype matches above threshold
- **THEN** the live turn SHALL display the formatted cluster label as today

### Requirement: Live speaker rename takes effect immediately (Fast mode)
During Fast-mode recording, the user SHALL be able to rename or reassign a live speaker via the same dropdown-or-text editor as offline transcripts. The editor SHALL default to single-turn scope: relabeling changes only the turn being edited (a per-turn override that persists to the matching transcript at stop). With the explicit "apply to all blocks of this speaker" option, the system SHALL instead update the session's cluster-to-person binding and merge the person's prototypes into the in-memory store, so subsequent chunks of that speaker are recognized and labeled for the remainder of the recording. Mid-recording rename in Efficient mode is NOT required (it has no live labels).

#### Scenario: Rename affects subsequent turns
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" mid-recording in Fast mode using the apply-to-all option
- **THEN** subsequent live turns recognized as that cluster SHALL display "Alice" for the rest of the session, and the binding SHALL be applied to final assignments at stop

#### Scenario: New name creates registry person mid-session
- **WHEN** user enters a name not present in the registry during a live rename
- **THEN** the system SHALL create the registry speaker immediately and use its (initially session-derived) prototypes for matching the remainder of the session

#### Scenario: In-place live relabel
- **WHEN** user renames a live speaker (single-turn or apply-to-all) while the live transcript panel is scrolled
- **THEN** only the affected turn(s) SHALL update their displayed name in place; the panel SHALL NOT fully re-render, SHALL keep the scroll position, and SHALL NOT flash an empty/loading state

### Requirement: Per-turn speaker override during live labeling
During Fast-mode recording, the system SHALL support relabeling a single live turn to a registry speaker (existing or newly created) without changing the session's cluster binding. The per-turn override SHALL be recorded in session memory, applied to the displayed turn immediately, and persisted at stop to the transcript matched to that turn; it SHALL NOT enroll embeddings (a single turn owns no cached embeddings) and SHALL NOT be overwritten by stop-time auto-recognition.

#### Scenario: Single misassigned turn corrected
- **WHEN** user relabels one live turn to "Bob" during a Fast-mode recording while the rest of its cluster stays linked to Alice
- **THEN** that turn SHALL display "Bob" immediately, the other turns of the cluster SHALL remain labeled as before, and at stop the transcript matched to that turn SHALL display "Bob"
