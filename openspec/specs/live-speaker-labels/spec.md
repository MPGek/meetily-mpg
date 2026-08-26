# live-speaker-labels Specification

## Purpose
TBD - created by archiving change live-speaker-labels-during-recording. Update Purpose after archive.
## Requirements
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

### Requirement: Live speaker labels use channel-stable IDs

The system SHALL label live speaker turns using the same channel-scoped ID scheme as offline diarization, determined once per session rather than per chunk, so a channel's label prefix never flips mid-recording and live labels always match the labels persisted at stop.

#### Scenario: Microphone turn labeled with mic prefix
- **WHEN** a stable turn originates from the microphone channel
- **THEN** the emitted `speaker` SHALL use the `MIC_SPEAKER_NN` scheme

#### Scenario: System turn labeled with system prefix
- **WHEN** a stable turn originates from the system channel
- **THEN** the emitted `speaker` SHALL use the `SPEAKER_NN` scheme

#### Scenario: Mono recording uses unprefixed scheme
- **WHEN** no system audio was captured during the recording
- **THEN** microphone turns SHALL be emitted with the `SPEAKER_NN` scheme

#### Scenario: Mic prefix stable for the whole session
- **WHEN** a recording captures microphone audio and system audio becomes available partway through the session
- **THEN** all microphone turns emitted before and after the first system chunk SHALL use the same `MIC_SPEAKER_NN` scheme, and the stop-time assignments SHALL use the same scheme

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

Live speaker assignment SHALL update only in-memory frontend state during recording; persistence of transcript speaker assignments SHALL still occur at recording stop through the existing `recording-stopped`/`speaker_assignments` path. Live rename operations MAY create registry speaker rows and update in-memory session state during recording, but SHALL NOT write transcript speaker assignments before stop. The stop-time assignment pass SHALL be reconciled with the session's cluster-to-person bindings (including live renames) so user-chosen identities are persisted rather than overwritten by raw cluster labels.

#### Scenario: No persistence during recording
- **WHEN** speaker labels are shown live during recording
- **THEN** no database write SHALL occur for transcript speaker assignments

#### Scenario: Stop-time assignments remain authoritative
- **WHEN** recording stops
- **THEN** the system SHALL compute and persist final speaker assignments via the existing stop-time path, reconciled with the session's cluster-to-person bindings (including live renames), so a cluster a user renamed mid-recording is saved under the user's chosen identity

#### Scenario: Live-renamed transcript keeps the user's name after save
- **WHEN** a user renames live cluster `SPEAKER_01` to "Alice" and recording stops
- **THEN** the saved transcripts of that cluster SHALL display "Alice" (user provenance) and SHALL NOT revert to the raw `SPEAKER_01` label

### Requirement: Live labels reflect recognized speakers
During Fast-mode recording, the system SHALL match each chunk embedding against an in-memory prototype store (loaded at recording start from the meeting's expected speakers, or all registry speakers when no list was provided) and SHALL display the recognized registry speaker's name on matching live turns instead of the raw cluster label.

#### Scenario: Known voice named live
- **WHEN** a Fast-mode recording has expected speaker Alice and an incoming chunk embedding matches Alice's prototypes above threshold
- **THEN** the live turn SHALL display "Alice"

#### Scenario: Unknown voice shows cluster label
- **WHEN** no prototype matches above threshold
- **THEN** the live turn SHALL display the formatted cluster label as today

### Requirement: Live speaker rename takes effect immediately (Fast mode)
During Fast-mode recording, the user SHALL be able to rename or reassign a live speaker via the same dropdown-or-text editor as offline transcripts. The editor SHALL default to single-turn scope: relabeling changes only the turn being edited (a per-turn override that persists to the matching transcript at stop), and the turn stream SHALL be rewritten so the edited turn and its matching transcript immediately display the user's name with user provenance rather than the prior auto name. With the explicit "apply to all blocks of this speaker" option, the system SHALL instead update the session's cluster-to-person binding, merge the person's prototypes into the in-memory store, and rewrite the live turn stream so all turns of that cluster carry the user's name for the remainder of the recording. Mid-recording rename in Efficient mode is NOT required (it has no live labels).

#### Scenario: Rename affects subsequent turns
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" mid-recording in Fast mode using the apply-to-all option
- **THEN** subsequent live turns recognized as that cluster SHALL display "Alice" for the rest of the session, and the binding SHALL be applied to final assignments at stop

#### Scenario: Rename affects already-emitted turns of the cluster
- **WHEN** user renames live speaker `SPEAKER_01` to "Alice" after several `SPEAKER_01` turns have already been emitted and shown with an auto name
- **THEN** those already-shown turns SHALL immediately display "Alice" with user provenance (no longer the auto name) for the remainder of the session

#### Scenario: New name creates registry person mid-session
- **WHEN** user enters a name not present in the registry during a live rename
- **THEN** the system SHALL create the registry speaker immediately and use its (initially session-derived) prototypes for matching the remainder of the session

#### Scenario: In-place live relabel
- **WHEN** user renames a live speaker (single-turn or apply-to-all) while the live transcript panel is scrolled
- **THEN** only the affected turn(s) SHALL update their displayed name in place; the panel SHALL NOT fully re-render, SHALL keep the scroll position, and SHALL NOT flash an empty/loading state

### Requirement: Per-turn speaker override during live labeling
During Fast-mode recording, the system SHALL support relabeling a single live turn to a registry speaker (existing or newly created) without changing the session's cluster binding. The per-turn override SHALL be recorded in session memory, applied to the displayed turn immediately, and persisted at stop to the transcript matched to that turn; it SHALL NOT be overwritten by stop-time auto-recognition. When the override's time window is covered by session embeddings, those embeddings SHALL be enrolled into the assigned speaker's global prototype set as ground truth.

#### Scenario: Single misassigned turn corrected
- **WHEN** user relabels one live turn to "Bob" during a Fast-mode recording while the rest of its cluster stays linked to Alice
- **THEN** that turn SHALL display "Bob" immediately, the other turns of the cluster SHALL remain labeled as before, and at stop the transcript matched to that turn SHALL display "Bob"

#### Scenario: Single-turn override enrolls ground truth
- **WHEN** user relabels one live turn to "Bob" and the turn's time window overlaps buffered session embeddings
- **THEN** the overlapping embeddings SHALL be enrolled as Bob's prototypes, in addition to the per-transcript override being persisted

### Requirement: User-assigned live labels are pinned
The live recording view SHALL treat a user-assigned speaker label as pinned for the rest of the session: later speaker-turn events SHALL NOT overwrite a pinned label's displayed name, even when the turn carries a different stale or auto-recognized display name. Only a further explicit user edit SHALL change a pinned label.

#### Scenario: Stale turn does not revert a pinned label
- **WHEN** user renames cluster `SPEAKER_01` to "Alice" and a later turn for that cluster arrives carrying a stale auto-recognized display name "Bob"
- **THEN** the transcript SHALL keep displaying "Alice" and SHALL NOT revert to "Bob"

#### Scenario: New cluster assignment still applies
- **WHEN** a later turn assigns a previously unpinned transcript to a different cluster that the user then renames
- **THEN** the transcript SHALL adopt the new cluster's label and the user's renamed display name for that cluster

### Requirement: Live user bindings persist deterministically on stop
When a live recording stops, the system SHALL persist user-assigned speaker identities into the stored data such that reopening the meeting deterministically shows the user's name for corrected blocks and clusters, independent of the render-time join alone. User-bound clusters SHALL be written to `meeting_speakers` with `matched_by='user'`, and single-turn overrides SHALL be written to the matching transcript's override, before the recording is considered finalized. Finalization SHALL run for every stopped recording that used live diarization, and a user binding SHALL NOT be silently dropped when the post-stop restore step cannot run.

#### Scenario: Corrected cluster shows the user's name after restart
- **WHEN** a user renames live cluster `SPEAKER_01` to "Alice" during a Fast-mode recording and then stops it
- **THEN** reopening the meeting SHALL show "Alice" for that cluster's transcripts with user provenance, without requiring any further user action after stop

#### Scenario: Single-turn override survives stop and reopen
- **WHEN** the user relabels one live turn to "Bob" and stops the recording
- **THEN** the transcript matched to that turn SHALL show "Bob" with user provenance after reopening

#### Scenario: Finalize always runs for live-diarized recordings
- **WHEN** a recording used live (Fast-mode) diarization and stops
- **THEN** the finalization SHALL run automatically and SHALL not be skipped based on a frontend flag that can be false when live bindings were made

#### Scenario: Assignment failure is not silent
- **WHEN** a user attempts a live speaker assignment but no live prototype store is active for the session
- **THEN** the assignment SHALL fail loudly (reported to the user) rather than succeed silently and later revert to a predicted label

