## MODIFIED Requirements

### Requirement: Online recognition at recording stop
At recording stop, both modes SHALL derive per-cluster embeddings (Efficient: from its clustering; Fast: by grouping buffered chunk embeddings by pipeline speaker ID via time overlap with stable turns), persist cluster centroids and exemplar caches, and auto-match clusters against candidate prototypes per the speaker-identity-registry recognition rules before emitting final speaker assignments. Final assignments SHALL be reconciled with the session's cluster-to-person bindings: a cluster the user bound during the session SHALL be saved under the user's chosen identity with `matched_by='user'`, and auto-recognition SHALL NOT overwrite it.

#### Scenario: Final assignments carry recognized names
- **WHEN** recording stops and a session cluster matches an expected speaker above threshold
- **THEN** the meeting's `meeting_speakers` row SHALL be linked with `matched_by='auto'`, and transcripts saved at stop SHALL display the recognized name

#### Scenario: Per-turn overrides applied at stop
- **WHEN** recording stops and a live turn carries a per-turn speaker override (from a single-turn relabel during Fast-mode recording)
- **THEN** the transcript matched to that turn SHALL display the override's speaker name, and stop-time auto-recognition SHALL NOT overwrite it

#### Scenario: Live-renamed cluster reconciled into assignments
- **WHEN** recording stops and a user had renamed live cluster `SPEAKER_01` to "Alice" mid-recording
- **THEN** the persisted `meeting_speakers` row for that cluster SHALL be `matched_by='user'` and the saved transcripts SHALL display "Alice", even though auto-recognition alone would have left them under the raw cluster label

### Requirement: Online enrollment at recording stop
At recording stop, the system SHALL enroll session cluster embeddings as prototypes for every cluster the user assigned during or after the session (including live renames), reparenting the session's cached embeddings per the speaker-identity-registry enrollment rules. Enrollment SHALL be keyed by the underlying pipeline speaker identity and channel so microphone and system voices are never mixed, and SHALL additionally cover the embeddings of any user-assigned transcript blocks as ground truth.

#### Scenario: Live rename persisted as voiceprint
- **WHEN** user renamed a live speaker to "Alice" during a Fast-mode recording and recording stops
- **THEN** that session cluster's buffered embeddings SHALL be enrolled as prototypes of Alice (best 8 by duration), in addition to the `meeting_speakers` binding with `matched_by='user'`

#### Scenario: Mic and system enrollments stay separate
- **WHEN** a session has a user-bound mic cluster and a system cluster that share the same numeric pipeline speaker index
- **THEN** the enrolled prototypes for each SHALL contain only embeddings from their own channel, and the two enrollments SHALL NOT contaminate each other

#### Scenario: Single-turn ground-truth block enrolled
- **WHEN** a user relabels a single live turn to "Bob" at stop
- **THEN** the embeddings overlapping that turn's time window SHALL be enrolled as Bob's prototypes alongside the per-transcript override

### Requirement: Fast mode extracts embeddings for recognition and enrollment
In Fast mode, the system SHALL run its own ResNet34 embedding extraction on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does. The buffered embeddings SHALL be attributable to a specific pipeline speaker identity and channel so they can be grouped for enrollment without mixing channels.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks

#### Scenario: Embeddings carry channel provenance
- **WHEN** buffered embeddings are grouped for enrollment at stop
- **THEN** each group SHALL be identifiable by channel and pipeline speaker identity, and groupings SHALL never span both channels