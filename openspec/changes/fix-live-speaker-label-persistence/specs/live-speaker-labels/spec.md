## MODIFIED Requirements

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