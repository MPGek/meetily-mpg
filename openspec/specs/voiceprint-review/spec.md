# voiceprint-review Specification

## Purpose

A Settings page that makes every speaker voiceprint visible and verifiable: a tree of confirmed speakers and their prototypes plus all unconfirmed meeting caches, each row showing its provenance and playing the original audio clip it was extracted from.

## Requirements

### Requirement: Voiceprint browser in Settings
The Settings UI SHALL expose a voiceprint browser that shows every speaker voiceprint stored in the database. Confirmed voiceprints SHALL be grouped under their registry speaker (with the speaker name, the `is_me` flag, and the prototype count). Unconfirmed cache embeddings SHALL be shown in a separate branch grouped by their source meeting. Each embedding row SHALL display its capture channel, source segment duration, source meeting and cluster label, and (when known) its time range, and SHALL indicate whether it is an enrolled prototype or an unconfirmed cache. Rows without provenance SHALL be shown with a "source unavailable" indicator rather than fabricating a meeting or times.

#### Scenario: Speakers listed with their prototypes
- **WHEN** the user opens the voiceprint browser and Alice has 8 enrolled prototypes
- **THEN** the browser SHALL show a speaker subtree for Alice with count 8, and each of her prototypes as a row listing channel, duration, and source (meeting + time range) where available

#### Scenario: Unconfirmed caches grouped by meeting
- **WHEN** the database contains unconfirmed cache embeddings from meetings M1 and M2
- **THEN** the browser SHALL show an unconfirmed branch with one subgroup per meeting M1 and M2, listing the meeting's cache rows with their channel, duration, and cluster label

#### Scenario: Both confirmed and unconfirmed are listed together
- **WHEN** the user opens the voiceprint browser
- **THEN** confirmed prototypes (under their speakers) and unconfirmed caches (under their meetings) SHALL be visible in the same view, since both are candidates for review or rejection

#### Scenario: Empty browser state
- **WHEN** there are no voiceprints at all
- **THEN** the browser SHALL show an empty state rather than an error

#### Scenario: Legacy row marked as source unavailable
- **WHEN** an enrolled prototype has no provenance fields
- **THEN** the row SHALL display a "source unavailable" indicator and no meeting/time information

### Requirement: Play original audio clip for an embedding
For any voiceprint row with a stored audio clip, the browser SHALL offer playback of that clip directly from the stored bytes without reading the meeting's audio file and without seeking by provenance timecodes. For rows without a stored clip but with provenance and a resolvable meeting audio file, the browser SHALL offer the legacy playback: stream the meeting recording through the existing media playback path (no full-file IPC transfer), seek to the row's `audio_start_time`, begin playback, and stop at `audio_end_time`. Rows with neither a stored clip nor a resolvable audio file with timecodes SHALL have playback disabled, showing an "audio unavailable" state for clip-less rows.

#### Scenario: Play clip with time range
- **WHEN** the user activates play on a provenanced prototype with `audio_start_time`=40.0 and `audio_end_time`=43.2 whose meeting audio resolves
- **THEN** playback SHALL start at 40.0 seconds of the meeting recording and automatically pause at 43.2 seconds

#### Scenario: Play disabled without audio file
- **WHEN** an embedding's meeting has no resolvable audio file
- **THEN** the browser SHALL show the play action disabled for that row

#### Scenario: Play disabled without timecodes
- **WHEN** an embedding has no `audio_start_time`/`audio_end_time`
- **THEN** the browser SHALL show the play action disabled for that row

#### Scenario: Clip playback stays streaming
- **WHEN** a long meeting recording plays a clip
- **THEN** playback SHALL use the streaming media path and SHALL NOT materialize the full recording in memory over IPC

#### Scenario: Stored clip plays without the meeting file
- **WHEN** the user activates play on a row with a stored clip whose meeting audio file is missing or whose timecodes are shifted
- **THEN** playback SHALL play the stored voice sample in full from its start, independent of the meeting file

### Requirement: Voiceprint browser storage summary
The voiceprint browser section SHALL display current storage totals — number of registry speakers, number of enrolled prototypes, number of unconfirmed cache embeddings, and total embedding bytes — consistent with the storage statistics command, and SHALL refresh after any review action (reject, reconfirm, or replacement).

#### Scenario: Counts match current usage
- **WHEN** the browser shows storage totals after a reject action
- **THEN** the totals SHALL reflect the post-reject prototype and cache counts

### Requirement: Verification state visible in browser
Each voiceprint row SHALL indicate whether it is verified or unverified, each speaker and meeting group header SHALL show its unverified count, and the browser SHALL offer a hide-verified toggle that filters the view to unverified rows only. Verification display SHALL refresh after any verify action without a page reload.

#### Scenario: Unverified badge on group
- **WHEN** Alice's group has 12 prototypes of which 3 are unverified
- **THEN** the group header SHALL show an unverified count of 3

#### Scenario: Hide verified filters rows
- **WHEN** hide-verified is enabled
- **THEN** verified rows SHALL be hidden and unverified rows SHALL remain grouped as before

### Requirement: Review actions exposed on rows
Each voiceprint row in the browser (confirmed prototype or unconfirmed cache) SHALL expose the reject and reconfirm actions whose behavior is defined by the voiceprint-rejection capability, and the browser SHALL surface confirmation and the affected-count report for global replacement before executing it. **The person-picker invoked for reconfirm and for whole-corpus replacement target selection SHALL be the same component as speaker assignment / rename**: a searchable list of existing registry persons with quick filter, an option for anonymous (no target), and an ability to create a new person inline, without a separate ad-hoc prompt.

#### Scenario: Actions present per row
- **WHEN** the user selects a voiceprint row
- **THEN** the row SHALL offer "reject" and "reconfirm to speaker" actions (and "replace speaker across meetings" for confirmed prototype sets)

#### Scenario: Reconfirm uses shared person picker
- **WHEN** the user reconfirms an unconfirmed cache
- **THEN** the browser SHALL open the shared person-picker dialog showing the searchable list of other persons with a quick-search filter and a create-new affordance

#### Scenario: Replace uses same dialog as name change
- **WHEN** the user initiates "replace speaker across meetings" for source speaker S
- **THEN** the browser SHALL open the same person-picker dialog as speaker name change / assignment, showing the searchable list of other persons (excluding S), quick-search filter, create-new person, and an explicit anonymous option; the chosen target (or anonymous) SHALL be used for the subsequent confirmation step

#### Scenario: Global replacement requires confirmation
- **WHEN** the user initiates a whole-corpus speaker replacement and has chosen a target (or anonymous) via the picker
- **THEN** the browser SHALL show the number of affected meetings, clusters, and transcripts and require explicit confirmation before proceeding

### Requirement: Voiceprint browser collapsible groups and bulk toggle
The voiceprint browser SHALL make each confirmed-speaker group and each unconfirmed-meeting group individually collapsible and expandable, and SHALL provide expand-all and collapse-all controls that affect all speaker and meeting groups at once. Collapsed state SHALL persist only for the current view session (no durable storage required), but the toggle SHALL be accessible, keyboard-operable, and reflect current state. Collapse/expand SHALL NOT alter which voiceprints are loaded or their provenance/playback behavior.

#### Scenario: Speaker group collapses and expands
- **WHEN** the user collapses the speaker group for Alice
- **THEN** Alice's prototype rows SHALL become hidden and a collapsed indicator SHALL be shown, and expanding the group SHALL reveal them again without reloading data

#### Scenario: Meeting group collapses and expands
- **WHEN** the user collapses the unconfirmed meeting group for M1
- **THEN** M1's cache rows SHALL become hidden and a collapsed indicator SHALL be shown, and expanding the group SHALL reveal them again

#### Scenario: Expand all reveals every group
- **WHEN** the user activates expand-all
- **THEN** every collapsed speaker group and every collapsed meeting group SHALL become expanded

#### Scenario: Collapse all hides every group
- **WHEN** the user activates collapse-all
- **THEN** every expanded speaker group and every expanded meeting group SHALL become collapsed, while the browser header, storage summary, and group counts SHALL remain visible

#### Scenario: Initial state is expanded
- **WHEN** the voiceprint browser first loads with voiceprints present
- **THEN** speaker and meeting groups SHALL be expanded by default so existing content remains immediately visible

#### Scenario: Empty groups respect collapse
- **WHEN** a speaker has zero prototypes or a meeting has no caches
- **THEN** its group header SHALL still render the collapsed/expanded affordance (or a disabled state) and expand/collapse actions SHALL NOT cause an error
