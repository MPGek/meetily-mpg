## Purpose

Letting users tag a recording before it starts and while it runs, so the saved meeting appears in Meeting Notes already labeled without post-hoc tagging.

## ADDED Requirements

### Requirement: Pre-start tag selection on the home screen

The home screen SHALL offer tag selection before recording starts: the user can tick existing tags and create new ones, forming a pending set for the upcoming recording. Starting the recording SHALL carry the pending set with the session.

#### Scenario: Select existing tags before start

- **WHEN** the user ticks `Work` and `Q3` on the home screen and starts recording
- **THEN** the pending set `{Work, Q3}` travels with the recording session.

#### Scenario: Create a new tag before start

- **WHEN** the user types a new name `Client` in the pre-start picker
- **THEN** the `Client` tag is created in the dictionary immediately and joins the pending set.

#### Scenario: Fresh recording starts empty

- **WHEN** a new recording starts without the user picking tags
- **THEN** its pending set is empty (no leakage from a previous recording).

### Requirement: Pending set editable during recording

While recording is in progress, the home screen SHALL allow editing the same pending set: adding/toggling existing tags, creating new ones, and removing previously selected ones. Edits SHALL affect only the pending set, never stored meetings (none exists yet).

#### Scenario: Add a tag mid-recording

- **WHEN** the user adds `Urgent` while recording with pending set `{Work}`
- **THEN** the pending set becomes `{Work, Urgent}` and the saved meeting will carry both.

#### Scenario: Remove a pre-selected tag mid-recording

- **WHEN** the user unticks `Work` while recording with pending set `{Work, Q3}`
- **THEN** the pending set becomes `{Q3}` and the saved meeting will not carry `Work`.

### Requirement: Pending tags persist when the meeting is saved

When the recording stops and the meeting is saved, the system SHALL link every tag of the pending set to the newly created meeting, so the meeting appears in Meeting Notes already tagged. Linking failures for individual tags SHALL NOT fail the meeting save; failures SHALL be surfaced to the user.

#### Scenario: Stop links pending tags

- **WHEN** a recording with pending set `{Work, Q3}` stops and saves successfully
- **THEN** the new meeting shows `Work` and `Q3` in the Meeting Notes list.

#### Scenario: Empty pending set changes nothing

- **WHEN** a recording with an empty pending set stops
- **THEN** the meeting is saved with no tags, exactly as recordings behave today.

#### Scenario: Partial link failure does not lose the meeting

- **WHEN** the meeting saves but linking one pending tag fails
- **THEN** the meeting and the successfully linked tags persist, and the user sees which tag failed.

### Requirement: Cancelled recordings clear the pending set

Cancelling or discarding a recording SHALL clear its pending set without creating any meeting or links. Dictionary tags created upfront SHALL remain (with zero usage), consistent with creating a tag anywhere else.

#### Scenario: Cancel clears pending state

- **WHEN** the user cancels a recording with pending set `{Work}`
- **THEN** no meeting and no links are created, and the next recording starts with an empty pending set.

#### Scenario: Pre-created tags survive cancellation

- **WHEN** the user created tag `Client` upfront and then cancels the recording
- **THEN** the `Client` tag still exists in the dictionary for future use.
