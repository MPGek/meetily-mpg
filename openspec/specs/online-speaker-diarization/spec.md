# online-speaker-diarization Specification

## Purpose
Speaker labels are assigned during recording (rather than only as a post-processing step) through selectable online diarization modes, giving users fast or efficient labeling while keeping microphone and system channels labeled with the same namespaced IDs used by offline diarization.
## Requirements
### Requirement: User can select diarization mode
The system SHALL allow users to choose between "Fast" (full streaming), "Efficient" (hybrid embedding + deferred clustering), and "Off" diarization modes via a dropdown in the diarization settings panel.

#### Scenario: User selects Fast mode
- **WHEN** user opens diarization settings and selects "Fast" from the mode dropdown
- **THEN** the setting is persisted to localStorage and subsequent recordings use full streaming diarization

#### Scenario: User selects Efficient mode
- **WHEN** user opens diarization settings and selects "Efficient" from the mode dropdown
- **THEN** the setting is persisted to localStorage and subsequent recordings use hybrid diarization (embeddings during recording, clustering at stop)

#### Scenario: User disables online diarization
- **WHEN** user opens diarization settings and selects "Off" from the mode dropdown
- **THEN** no diarization processing occurs during recording; offline diarization remains available as an explicit post-recording action

### Requirement: Efficient mode extracts speaker embeddings during recording
The system SHALL extract speaker embeddings from VAD-detected speech segments during recording when Efficient mode is selected, buffering them in memory without performing clustering.

#### Scenario: Embedding extracted from speech segment
- **WHEN** the VAD detects a speech segment of at least 200ms duration during recording in Efficient mode
- **THEN** the system SHALL resample the segment to 16 kHz if needed and extract a speaker embedding vector using the polyvoice `ResNet34Adapter`, storing it with the segment's start and end timestamps

#### Scenario: Silence periods skipped
- **WHEN** the VAD detects silence (no speech) for more than 500ms during recording in Efficient mode
- **THEN** the system SHALL NOT extract embeddings for the silence period

### Requirement: Efficient mode clusters embeddings at recording stop
The system SHALL trigger speaker clustering on all buffered embeddings when recording stops in Efficient mode, producing per-transcript speaker assignments.

#### Scenario: Clustering completes at recording stop
- **WHEN** recording stops in Efficient mode with buffered embeddings from the session
- **THEN** the system SHALL run `AhcClusterer` on all buffered embeddings, map resulting speaker labels to transcript segments by temporal overlap, and update the meeting's transcripts via the same DB path as offline diarization

#### Scenario: No speech detected during recording
- **WHEN** recording stops in Efficient mode but no speech segments were detected (empty embedding buffer)
- **THEN** the system SHALL mark diarization as "no-speech" without error and skip transcript updates

### Requirement: Fast mode runs full streaming diarization during recording

The system SHALL use the polyvoice `StreamingPipeline` to perform segmentation, embedding extraction, and incremental speaker caching continuously during recording when Fast mode is selected.

#### Scenario: Streaming diarization processes audio chunks
- **WHEN** audio chunks arrive during recording in Fast mode
- **THEN** the system SHALL feed the VAD-detected 16 kHz speech chunks into the `StreamingPipeline`, which outputs speaker-labeled turns as they become available

#### Scenario: Speaker segments buffered internally
- **WHEN** the `StreamingPipeline` outputs a stable speaker turn during recording in Fast mode
- **THEN** the system SHALL buffer the turn internally for the final stop-time assignment pass

#### Scenario: Stable turns emitted live to frontend
- **WHEN** the `StreamingPipeline` outputs a stable speaker turn during recording in Fast mode
- **THEN** the system SHALL translate the turn to absolute recording time, emit it to the frontend via the `online-speaker-turn` event, and still buffer the turn for the final stop-time assignment pass

### Requirement: Speaker labels written at recording stop (both modes)
The system SHALL assign speaker labels to all transcripts at recording stop, regardless of which online mode was used, using the same `update_transcript_speaker` database path as offline diarization.

#### Scenario: Transcripts updated with speaker IDs
- **WHEN** recording stops and online diarization has produced speaker segment assignments (Fast mode) or embeddings have been clustered (Efficient mode)
- **THEN** the system SHALL compute speaker-to-transcript matches by temporal overlap, update each transcript's speaker field in the database, and update the meeting's `diarization_status` to "complete"

#### Scenario: Speaker labels separated by channel
- **WHEN** speaker matching runs on transcripts after online diarization
- **THEN** transcripts with `source_device = "System"` SHALL be matched against system-channel diarization results and assigned `SPEAKER_NN` IDs, and transcripts with `source_device = "Microphone"` SHALL be matched against microphone-channel results and assigned `MIC_SPEAKER_NN` IDs, consistent with offline diarization behavior

### Requirement: Online diarization processes channels separately
The system SHALL keep microphone and system audio in separate diarization pipelines during recording, so local and remote speakers are labeled independently with the same namespaced IDs used by offline diarization (`MIC_SPEAKER_NN` for mic, `SPEAKER_NN` for system).

#### Scenario: Efficient mode buffers embeddings per channel
- **WHEN** an audio chunk arrives on the embedding channel in Efficient mode
- **THEN** the system SHALL route the chunk's embedding into a buffer keyed by its `device_type` (microphone or system), keeping two independent embedding buffers

#### Scenario: Fast mode runs one streaming diarizer per channel
- **WHEN** Fast mode is active
- **THEN** the system SHALL maintain a separate `SpeakerDiarization` instance for each channel and feed each instance only audio from its own channel

#### Scenario: Clustering produces namespaced IDs
- **WHEN** recording stops and clustering runs on the buffered embeddings (Efficient mode) or buffered segments (Fast mode)
- **THEN** clusters from the microphone buffer SHALL map to `MIC_SPEAKER_NN` and clusters from the system buffer SHALL map to `SPEAKER_NN`

### Requirement: Graceful fallback to offline diarization
The system SHALL fall back to the existing offline diarization pipeline if online processing fails for any reason.

#### Scenario: Streaming diarization initialization failure
- **WHEN** the polyvoice `StreamingPipeline` fails to initialize (e.g., model download failure) in Fast mode
- **THEN** the system SHALL log the error, emit a warning event to the frontend, and continue recording without diarization; offline diarization remains available after recording stops

#### Scenario: Embedding extraction runtime error
- **WHEN** the polyvoice embedding model fails during recording in Efficient mode
- **THEN** the system SHALL log the error, clear the embedding buffer, and allow offline diarization to run after recording stops

### Requirement: Multiple recordings cannot run online diarization simultaneously
The system SHALL enforce that at most one online diarization session is active at a time, reusing the existing `DiarizationGuard` pattern.

#### Scenario: Second recording blocked from online diarization
- **WHEN** a recording is already in progress with online diarization active and a second recording attempts to start with online diarization enabled
- **THEN** the second recording SHALL proceed without online diarization (transcription-only), and the frontend SHALL be notified that online diarization is unavailable

### Requirement: Efficient mode clusters with the calibrated threshold

The system SHALL cluster the buffered speaker embeddings at recording stop in Efficient mode using the same fixed cosine-similarity threshold (`0.45`) as offline diarization, so distinct speakers are assigned distinct labels and online/offline results stay consistent.

#### Scenario: Multi-speaker recording produces distinct labels

- **WHEN** recording stops in Efficient mode with buffered embeddings containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and matched transcripts SHALL be assigned distinct speaker IDs

#### Scenario: Clustering matches offline label scheme

- **WHEN** an online-Efficient-diarized meeting is re-analyzed with offline diarization
- **THEN** both paths SHALL apply the same fixed threshold, producing the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and comparable speaker counts

### Requirement: Efficient mode prunes singleton clusters

The system SHALL dissolve single-embedding clusters in Efficient mode by reassigning them to the nearest larger cluster, matching offline diarization behavior.

#### Scenario: Singleton fragment reassigned

- **WHEN** Efficient-mode clustering produces a cluster containing fewer than two embeddings
- **THEN** the system SHALL reassign those embeddings to the nearest cluster with at least two embeddings

### Requirement: Gap-fill matches offline behavior

The system SHALL fill unmatched transcripts with the nearest speaker at recording stop, using the same rules as offline diarization.

#### Scenario: Short mic utterance labeled

- **WHEN** a microphone transcript has no overlapping diarization segment and the microphone channel has a single speaker
- **THEN** the transcript SHALL be assigned that speaker's `MIC_SPEAKER_NN` label

#### Scenario: Gap-fill bounded on multi-speaker channel

- **WHEN** a transcript has no overlapping diarization segment and the channel has multiple speakers
- **THEN** the transcript SHALL be assigned the temporally nearest segment's speaker when within 30 seconds, and SHALL remain NULL otherwise

### Requirement: Expected speakers provided at recording start
The system SHALL accept an optional list of expected registry speaker IDs when online diarization recording starts. Because the meeting row does not exist until recording stop, the list SHALL be held in session memory during recording and SHALL be persisted to `meeting_expected_speakers` when the meeting is created at stop.

#### Scenario: List persisted at stop
- **WHEN** recording starts with expected speakers {Alice, Bob} and stops successfully
- **THEN** the created meeting SHALL have `meeting_expected_speakers` rows for Alice and Bob

#### Scenario: No list means match all
- **WHEN** recording starts without an expected-speaker list
- **THEN** recognition during and after the session SHALL consider all registry speakers

### Requirement: Fast mode extracts embeddings for recognition and enrollment
In Fast mode, the system SHALL run its own ResNet34 embedding extraction on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does. The buffered embeddings SHALL be attributable to a specific pipeline speaker identity and channel so they can be grouped for enrollment without mixing channels.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks

#### Scenario: Embeddings carry channel provenance
- **WHEN** buffered embeddings are grouped for enrollment at stop
- **THEN** each group SHALL be identifiable by channel and pipeline speaker identity, and groupings SHALL never span both channels

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

