# meeting-list-display Specification

## Purpose

Compact, scannable rows in the Meeting Notes list showing when each recording was made and how it is labeled, so users can distinguish many meetings at a glance.

## Requirements

### Requirement: Row shows recording date and time in fixed format

Each Meeting Notes row SHALL display the meeting's creation timestamp formatted as `yyyy-mm-dd hh:mm` in 24-hour notation in the user's local timezone, below the meeting title.

#### Scenario: Standard row shows date

- **WHEN** a meeting created at local time `2026-09-11 14:30` appears in the list
- **THEN** the row shows `2026-09-11 14:30` in the meta line under the title.

#### Scenario: Midnight and noon edges

- **WHEN** meetings created at local `00:05` and `12:00` are listed
- **THEN** they render as `hh:mm` `00:05` and `12:00` respectively (never `12:05 AM` or empty).

#### Scenario: Invalid timestamp never breaks the list

- **WHEN** a meeting record has a missing or unparsable `created_at`
- **THEN** the row still renders the title and tags with a fallback placeholder in the date slot instead of hiding the row.

### Requirement: Compact typography fits more content

Meeting rows SHALL use compact type (title smaller than the current `text-sm`, meta line smaller still) with the title taking the full row width and wrapping onto up to three lines, so long titles stay readable without expanding the sidebar width. Action buttons (tag editor, rename, delete) SHALL live on the date/tags meta line — never on the title line — so they do not crowd or obscure the title.

#### Scenario: Long title wraps up to three lines

- **WHEN** a meeting title exceeds the row width
- **THEN** the title wraps onto up to three full-width lines with no ellipsis, and a title longer than three lines clamps with ellipsis plus the full title available via tooltip or navigation to the meeting.

#### Scenario: Title fully readable

- **WHEN** the user looks at any meeting row
- **THEN** the title text is never truncated on short titles and never overlapped by action buttons.

#### Scenario: Actions sit on the meta line

- **WHEN** the user hovers or focuses a meeting row
- **THEN** the tag, rename, and delete buttons appear on the date/tags meta line (date left, buttons right), and the title line contains only the title.

### Requirement: Row displays meeting tags with overflow cap

Each row SHALL render the meeting's tags as compact colored pills and SHALL cap the visible count, collapsing the remainder into a `+M` indicator with full list available on hover or focus.

#### Scenario: Few tags fully shown

- **WHEN** a meeting has two tags
- **THEN** both pills render inline under the date line.

#### Scenario: Many tags collapse

- **WHEN** a meeting has more tags than the visible cap
- **THEN** the row shows the first N pills plus `+M` for the remainder, and hovering reveals the full set.

#### Scenario: Tag color visible

- **WHEN** tags `Work` (blue) and `Q3` (amber) are linked to a meeting
- **THEN** each pill renders in its tag color with readable contrast text.

#### Scenario: No tags keeps clean row

- **WHEN** a meeting has no tags
- **THEN** the row shows title plus date with no empty tag strip reserving vertical space.

### Requirement: List order and search include tags without breaking existing behavior

The list SHALL remain ordered by `created_at` descending, and typing in the existing search box SHALL also match tag names in addition to titles and transcript content, without removing current title/transcript matching.

#### Scenario: Order unchanged

- **WHEN** meetings exist with different creation times
- **THEN** the newest meeting appears first regardless of tags.

#### Scenario: Search matches tag

- **WHEN** user searches `work` and a meeting is tagged `Work` but its title and transcript do not contain `work`
- **THEN** that meeting appears in the filtered list.

#### Scenario: Existing search preserved

- **WHEN** user searches text that matches only a transcript snippet
- **THEN** the transcript-match highlight behavior works exactly as before.
