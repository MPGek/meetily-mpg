## Purpose

Present transcript speech to the reader as speaker turns, not as audio chunks between pauses, so a person who spoke continuously is not shown as a stack of records all labeled with the same name. This applies to both the live recording view and the saved meeting-details view without altering stored data.

## ADDED Requirements

### Requirement: Consecutive same-speaker segments form one displayed turn
The transcript views (live recording view and meeting-details view) SHALL merge runs of chronologically adjacent transcript segments that share the same resolved speaker value on the same `source_device` into a single displayed turn block.

#### Scenario: Two consecutive records of one speaker are shown as one block
- **WHEN** two transcript segments with the same resolved speaker, the same `source_device`, and no unresolved gap between them are adjacent in chronological order
- **THEN** the transcript view SHALL render them as one turn block instead of two separate records

#### Scenario: Speaker change starts a new turn
- **WHEN** a segment with a different resolved speaker follows a segment in chronological order
- **THEN** the transcript view SHALL start a new turn block at the differing segment

#### Scenario: Channel change starts a new turn
- **WHEN** a segment with the same resolved speaker but a different `source_device` follows another segment
- **THEN** the transcript view SHALL start a new turn block at that segment (e.g., Microphone and System audio with identical speaker values are never merged)

### Requirement: Unresolved-speaker segments are not merged
The transcript views SHALL NOT merge a segment whose speaker is unresolved or missing into any turn; an unresolved segment SHALL render as its own record.

#### Scenario: Segment without speaker stays separate
- **WHEN** a transcript segment has no resolved speaker value (no speaker event matched it yet)
- **THEN** the transcript view SHALL render it as its own record, even when adjacent segments share the same resolved speaker

### Requirement: Long-silence gap starts a new turn
The transcript views SHALL start a new turn block when the gap between the end of a same-speaker segment and the start of the next same-speaker segment on the same channel reaches 60 seconds.

#### Scenario: Gap of 60 seconds or more splits a turn
- **WHEN** two same-speaker same-channel segments are separated by 60 seconds or more of unattributed time
- **THEN** the transcript view SHALL render them in separate turn blocks

#### Scenario: Small pause does not split a turn
- **WHEN** two same-speaker same-channel segments are separated by less than 60 seconds
- **THEN** the transcript view SHALL keep them in a single turn block

### Requirement: Merged turn span covers its member segments
A merged turn block SHALL use the start time of its first member segment as the turn start and the end time of its last member segment as the turn end, so the play button seeks to the beginning of the utterance.

#### Scenario: Play button seeks to utterance start
- **WHEN** the user activates playback on a merged turn block
- **THEN** the audio player SHALL seek to the start time of the turn's first member segment

#### Scenario: Turn highlight during playback follows member spans
- **WHEN** audio playback is inside the time window of a member segment of a merged turn
- **THEN** the transcript view SHALL highlight that turn block as active

### Requirement: Speaker labels under merged rendering follow existing assignment logic
The displayed speaker name within a merged turn SHALL continue to be computed by the existing speaker labeling logic (`live-speaker-labels` for the live view, persisted assignments for the meeting-details view). Turn merging SHALL NOT advance, revert, or alter any speaker assignment or user pin.

#### Scenario: Pinned live label is respected inside a merged turn
- **WHEN** a user pins a speaker name for a turn during a live recording and adjacent segments merge into that turn
- **THEN** the merged turn SHALL display the pinned name and remain subject to live speaker-label update rules unchanged

#### Scenario: A late speaker turn event re-forms the merge
- **WHEN** during a live recording new transcript segments arrive or speaker-turn events change which speaker is assigned to adjacent segments
- **THEN** the displayed grouping SHALL be recomputed from the current transcript list and turn events so that segments may move between turns, and previously merged members MAY split when their resolved speakers diverge

### Requirement: Merged text preserves member records
A merged turn block SHALL render the concatenated text of its member segments in chronological order, preserving each member's original content.

#### Scenario: Members retain their text
- **WHEN** a merged turn block is displayed
- **THEN** its text SHALL contain the full text of every member segment in chronological order, and the underlying records SHALL remain retrievable (e.g., for editing, re-summarization, and word-level alignment rows)

### Requirement: Word-level sub-rows keep their member's source side

When a member segment of a merged turn has live word-level diarization sub-rows (`blocks`, live Fast mode), the transcript views SHALL render those sub-rows inside the turn on that member's `source_device` side, using the same side cues as single-record rendering (content alignment, label placement, and timestamp side per `split-transcript-ui`), and SHALL NOT place them on the opposite side. The container's side SHALL follow the member's `source_device`, not the cluster label of the sub-row.

#### Scenario: Split System member stays on the System side
- **WHEN** a merged turn member has `source_device` "System" and live word-level diarization split it into more than one speaker run
- **THEN** the transcript view SHALL render that member's sub-rows on the System side of the turn row, with the System side's alignment and label placement

#### Scenario: Split Microphone member stays on the Microphone side
- **WHEN** a merged turn member has `source_device` "Microphone" and live word-level diarization split it into more than one speaker run
- **THEN** the transcript view SHALL render that member's sub-rows on the Microphone side of the turn row

#### Scenario: A sub-row label does not move the row to the other side
- **WHEN** a split member's sub-row carries a cluster label that differs from the turn's displayed speaker (for example a System cluster shown as "Speaker 1" inside a turn headed by another System cluster)
- **THEN** the sub-row SHALL keep its own label on the member's source side and the row SHALL NOT be placed on the opposite side of the turn
