# meeting-tags Specification

## Purpose

User-defined color labels for grouping and visually distinguishing meeting recordings across the Meeting Notes list.

## Requirements

### Requirement: Tag dictionary with case-insensitive unique names

The system SHALL maintain a global dictionary of tags where each tag has a non-empty name and a color, and tag names SHALL be unique case-insensitively after trimming surrounding whitespace.

#### Scenario: Create new tag

- **WHEN** user creates a tag with name `  Work  `
- **THEN** the system stores it as `Work` and returns it in the tag list.

#### Scenario: Duplicate name rejected

- **WHEN** user creates a tag named `work` while `Work` already exists
- **THEN** the system rejects the duplicate and surfaces the existing tag instead of creating a second row.

#### Scenario: Empty name rejected

- **WHEN** user attempts to create a tag with an empty or whitespace-only name
- **THEN** the system rejects the request with a validation error and creates nothing.

### Requirement: Deterministic default color from fixed palette with manual override

The system SHALL assign every new tag a default color selected deterministically from a fixed palette of at least 8 colors, and SHALL allow the color to be changed afterwards without renaming the tag.

#### Scenario: Default color assigned

- **WHEN** user creates a tag without specifying a color
- **THEN** the system assigns a palette color deterministically so the same tag name always resolves to the same default color on a fresh database.

#### Scenario: Palette cycles

- **WHEN** more tags are created than there are palette entries
- **THEN** the system cycles through the palette rather than failing or reusing only one color.

#### Scenario: Manual color override

- **WHEN** user changes the color of an existing tag
- **THEN** the system persists the new color and all meetings showing that tag display the new color.

### Requirement: Assign and unassign tags on meetings

The system SHALL support a many-to-many link between meetings and tags, allowing zero or more tags per meeting, assigning an existing tag, creating-and-assigning a new tag in one action, and removing a tag from a meeting without deleting the tag itself.

#### Scenario: Assign existing tag

- **WHEN** user assigns existing tag `Work` to a meeting that does not have it
- **THEN** the meeting shows `Work` in its tag list and the tag remains available for other meetings.

#### Scenario: Create and assign in one step

- **WHEN** user types a new name `Q3` in the meeting tag editor
- **THEN** the system creates the `Q3` tag with a default palette color and links it to that meeting.

#### Scenario: Unassign keeps dictionary entry

- **WHEN** user removes tag `Work` from a meeting
- **THEN** the meeting no longer shows `Work` but the `Work` tag still exists in the dictionary.

#### Scenario: Idempotent assign

- **WHEN** user assigns a tag that is already linked to the meeting
- **THEN** the system leaves a single link (no duplicates) and reports success.

### Requirement: Rename, delete, and list tags

The system SHALL allow renaming a tag (preserving all meeting links), deleting a tag (removing all its meeting links), and listing all tags with usage counts ordered for autocomplete.

#### Scenario: Rename preserves links

- **WHEN** user renames tag `Work` to `Client work`
- **THEN** every meeting previously tagged `Work` now shows `Client work` with the same color unless the color was changed separately.

#### Scenario: Delete removes links

- **WHEN** user deletes tag `Work`
- **THEN** the tag disappears from the dictionary and from every meeting that had it, while the meetings themselves are untouched.

#### Scenario: List for autocomplete

- **WHEN** user opens the tag editor and types `wo`
- **THEN** the system suggests matching existing tags (e.g. `Work`) ordered by usage so the user can pick instead of creating a duplicate.

### Requirement: Tag links follow meeting lifecycle

The system SHALL delete all tag links of a meeting when that meeting is deleted, and SHALL NOT leave orphaned link rows.

#### Scenario: Meeting deletion cascades links

- **WHEN** user deletes a meeting that has two tags
- **THEN** the meeting and its tag links are removed while the tag dictionary entries remain.
