# Spec Delta

## MODIFIED Requirements

### Requirement: Frontend matches speaker turns to live transcripts

The frontend SHALL assign each live transcript segment a speaker by matching its recording-relative time window against emitted speaker turns from the same channel, selecting the turn with the greatest temporal overlap. Matching SHALL happen both when a turn arrives and when a segment arrives, so the label does not depend on which of the two arrives first.

#### Scenario: Overlapping turn assigns speaker
- **WHEN** a transcript segment's `[audio_start_time, audio_end_time]` overlaps an emitted turn on the same channel
- **THEN** the segment's `speaker` SHALL be set to that turn's speaker

#### Scenario: Retroactive label fill-in
- **WHEN** a transcript segment is rendered before the speaker turn covering its time window becomes stable and is emitted
- **THEN** the segment's `speaker` SHALL be set once the turn arrives, updating the already-rendered segment

#### Scenario: Turn emitted before its segment arrives
- **WHEN** a speaker turn on a channel has already been emitted and a transcript segment on the same channel whose time window overlaps that turn arrives afterwards, with no further turn emitted
- **THEN** the segment SHALL be rendered with that turn's `speaker` without waiting for another speaker turn

#### Scenario: Pinned segment not re-matched on arrival
- **WHEN** a transcript segment already carries a user-assigned (pinned) label and is re-delivered or re-sorted with newly arrived segments
- **THEN** the arrival-time matching SHALL NOT change its label

#### Scenario: No matching turn leaves speaker unset
- **WHEN** no emitted turn overlaps a transcript segment's time window
- **THEN** the segment's `speaker` SHALL remain unset
