## ADDED Requirements

### Requirement: Pending-set durability and visible failures

The system SHALL persist the pending tag set of the active recording on every selection change (add, remove, create) and SHALL carry a pre-start selection into the recording session. When persisting or carrying the set fails, the system MUST keep the user's selection in the picker and MUST surface a user-visible warning; the pending set MUST NOT be silently reset to empty.

#### Scenario: Pre-start selection carries into the recording

- **WHEN** the user selects `Work` before start and starts the recording
- **THEN** the recording's stored pending set contains `Work` and the picker still shows `Work` while recording

#### Scenario: Mid-recording edit persists before stop

- **WHEN** the user adds `Urgent` while recording
- **THEN** the stored pending set contains `Urgent` before the recording stops

#### Scenario: Persist failure is surfaced

- **WHEN** persisting the pending set fails, for example because the recording folder is not available yet
- **THEN** the picker keeps the user's selection and the user sees a warning, and the selection is not silently cleared

#### Scenario: Reload mid-recording keeps the set

- **WHEN** the UI reloads during recording after tags were selected
- **THEN** the picker shows the stored pending set

### Requirement: Pending-set changes never reset silently

The system SHALL NOT replace the visible pending set with an empty set unless the user cleared it, the recording was cancelled or discarded, or the recording was saved. A read that cannot reach the active recording MUST preserve the current selection.

#### Scenario: Unreadable active recording preserves selection

- **WHEN** reading the pending set fails transiently while recording
- **THEN** the picker keeps the current selection instead of showing an empty set

#### Scenario: Save clears the set after linking

- **WHEN** the recording stops and the meeting is saved successfully
- **THEN** the pending set is cleared only after the tags have been linked to the meeting

## MODIFIED Requirements

### Requirement: Pending tags persist when the meeting is saved

When the recording stops and the meeting is saved, the system SHALL link every tag of the pending set to the newly created meeting, so the meeting appears in Meeting Notes already tagged. Linking failures for individual tags SHALL NOT fail the meeting save; failures SHALL be surfaced to the user. The system SHALL resolve the recording folder from the finished recording rather than depending solely on frontend session state, and it SHALL NOT drop a non-empty pending set without a user-visible warning.

#### Scenario: Stop links pending tags

- **WHEN** a recording with pending set `{Work, Q3}` stops and saves successfully
- **THEN** the new meeting shows `Work` and `Q3` in the Meeting Notes list

#### Scenario: Empty pending set changes nothing

- **WHEN** a recording with an empty pending set stops
- **THEN** the meeting is saved with no tags, exactly as recordings behave today

#### Scenario: Partial link failure does not lose the meeting

- **WHEN** the meeting saves but linking one pending tag fails
- **THEN** the meeting and the successfully linked tags persist, and the user sees which tag failed

#### Scenario: Save without frontend folder state still links

- **WHEN** the meeting is saved but the frontend does not provide the recording folder path, for example because its session state was lost
- **THEN** the system resolves the recording folder from the finished recording and still links the pending tags

#### Scenario: Non-empty set with no resolvable folder warns

- **WHEN** the pending set is non-empty but the recording folder cannot be resolved at save time
- **THEN** the meeting is saved and the user sees a warning that the tags were not linked
