# Spec delta: online-speaker-diarization

## ADDED Requirements

### Requirement: Expected speakers provided at recording start
The system SHALL accept an optional list of expected registry speaker IDs when online diarization recording starts. Because the meeting row does not exist until recording stop, the list SHALL be held in session memory during recording and SHALL be persisted to `meeting_expected_speakers` when the meeting is created at stop.

#### Scenario: List persisted at stop
- **WHEN** recording starts with expected speakers {Alice, Bob} and stops successfully
- **THEN** the created meeting SHALL have `meeting_expected_speakers` rows for Alice and Bob

#### Scenario: No list means match all
- **WHEN** recording starts without an expected-speaker list
- **THEN** recognition during and after the session SHALL consider all registry speakers

### Requirement: Fast mode extracts embeddings for recognition and enrollment
In Fast mode, the system SHALL run its own ResNet34 embedding extraction on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks

### Requirement: Online recognition at recording stop
At recording stop, both modes SHALL derive per-cluster embeddings (Efficient: from its clustering; Fast: by grouping buffered chunk embeddings by pipeline speaker ID via time overlap with stable turns), persist cluster centroids and exemplar caches, and auto-match clusters against candidate prototypes per the speaker-identity-registry recognition rules before emitting final speaker assignments.

#### Scenario: Final assignments carry recognized names
- **WHEN** recording stops and a session cluster matches an expected speaker above threshold
- **THEN** the meeting's `meeting_speakers` row SHALL be linked with `matched_by='auto'`, and transcripts saved at stop SHALL display the recognized name

#### Scenario: Per-turn overrides applied at stop
- **WHEN** recording stops and a live turn carries a per-turn speaker override (from a single-turn relabel during Fast-mode recording)
- **THEN** the transcript matched to that turn SHALL display the override's speaker name, and stop-time auto-recognition SHALL NOT overwrite it

### Requirement: Online enrollment at recording stop
At recording stop, the system SHALL enroll session cluster embeddings as prototypes for every cluster the user assigned during or after the session (including live renames), reparenting the session's cached embeddings per the speaker-identity-registry enrollment rules.

#### Scenario: Live rename persisted as voiceprint
- **WHEN** user renamed a live speaker to "Alice" during a Fast-mode recording and recording stops
- **THEN** that session cluster's buffered embeddings SHALL be enrolled as prototypes of Alice (best 8 by duration), in addition to the `meeting_speakers` binding with `matched_by='user'`
