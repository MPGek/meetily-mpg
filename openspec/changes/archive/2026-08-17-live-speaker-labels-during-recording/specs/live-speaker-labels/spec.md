## ADDED Requirements

### Requirement: Fast mode emits stable speaker turns live

The system SHALL emit a speaker turn to the frontend during recording, in Fast mode, as soon as the polyvoice `StreamingPipeline` reports the turn as stable.

#### Scenario: Stable turn emitted with absolute times
- **WHEN** the `StreamingPipeline` outputs a stable speaker turn during recording in Fast mode
- **THEN** the system SHALL translate the turn's start and end from pipeline time to absolute recording time and emit an `online-speaker-turn` event carrying `start_time`, `end_time`, `speaker`, and `source_device`

#### Scenario: Non-stable turns not emitted
- **WHEN** the `StreamingPipeline` outputs a turn that is not yet stable
- **THEN** the system SHALL NOT emit an `online-speaker-turn` event for that turn

#### Scenario: No live turns in other modes
- **WHEN** the online diarization mode is Efficient or Off
- **THEN** the system SHALL NOT emit any `online-speaker-turn` events during recording

### Requirement: Live speaker turns carry channel-scoped IDs

The system SHALL label live speaker turns using the same channel-scoped ID scheme as offline diarization.

#### Scenario: Microphone turn labeled with mic prefix
- **WHEN** a stable turn originates from the microphone channel
- **THEN** the emitted `speaker` SHALL use the `MIC_SPEAKER_NN` scheme

#### Scenario: System turn labeled with system prefix
- **WHEN** a stable turn originates from the system channel
- **THEN** the emitted `speaker` SHALL use the `SPEAKER_NN` scheme

#### Scenario: Mono recording uses unprefixed scheme
- **WHEN** no system audio was captured during the recording
- **THEN** microphone turns SHALL be emitted with the `SPEAKER_NN` scheme

### Requirement: Frontend matches speaker turns to live transcripts

The frontend SHALL assign each live transcript segment a speaker by matching its recording-relative time window against emitted speaker turns from the same channel, selecting the turn with the greatest temporal overlap.

#### Scenario: Overlapping turn assigns speaker
- **WHEN** a transcript segment's `[audio_start_time, audio_end_time]` overlaps an emitted turn on the same channel
- **THEN** the segment's `speaker` SHALL be set to that turn's speaker

#### Scenario: Retroactive label fill-in
- **WHEN** a transcript segment is rendered before the speaker turn covering its time window becomes stable and is emitted
- **THEN** the segment's `speaker` SHALL be set once the turn arrives, updating the already-rendered segment

#### Scenario: No matching turn leaves speaker unset
- **WHEN** no emitted turn overlaps a transcript segment's time window
- **THEN** the segment's `speaker` SHALL remain unset

### Requirement: Recording page displays live speaker labels

The recording transcript panel SHALL pass each segment's `speaker` value into the transcript renderer so a colored dot and speaker label are shown during recording.

#### Scenario: Segment with speaker shows label
- **WHEN** a live transcript segment has a `speaker` value
- **THEN** the recording page SHALL render that segment with a speaker dot and label using the speaker's color

#### Scenario: Segment without speaker shows no label
- **WHEN** a live transcript segment has no `speaker` value
- **THEN** the recording page SHALL render the segment without speaker labeling

### Requirement: Live labels are display-only until stop

Live speaker assignment SHALL update only in-memory frontend state during recording; persistence SHALL still occur at recording stop through the existing `recording-stopped`/`speaker_assignments` path.

#### Scenario: No persistence during recording
- **WHEN** speaker labels are shown live during recording
- **THEN** no database write SHALL occur for those speaker assignments

#### Scenario: Stop-time assignments remain authoritative
- **WHEN** recording stops
- **THEN** the system SHALL compute and persist final speaker assignments via the existing stop-time path, overwriting any transient live label
