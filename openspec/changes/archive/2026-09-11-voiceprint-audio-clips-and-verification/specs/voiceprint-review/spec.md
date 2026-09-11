## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: Verification state visible in browser
Each voiceprint row SHALL indicate whether it is verified or unverified, each speaker and meeting group header SHALL show its unverified count, and the browser SHALL offer a hide-verified toggle that filters the view to unverified rows only. Verification display SHALL refresh after any verify action without a page reload.

#### Scenario: Unverified badge on group
- **WHEN** Alice's group has 12 prototypes of which 3 are unverified
- **THEN** the group header SHALL show an unverified count of 3

#### Scenario: Hide verified filters rows
- **WHEN** hide-verified is enabled
- **THEN** verified rows SHALL be hidden and unverified rows SHALL remain grouped as before
