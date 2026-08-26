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
- **THEN** the system SHALL resample the segment to 16 kHz if needed and extract a speaker embedding vector using the enhanced TitaNet-Large embedder (192-d, layout-correct for `titanet_large`), storing it with the segment's start and end timestamps

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
The system SHALL fall back to the existing offline diarization pipeline if online processing fails for any reason. Online initialization SHALL use the same 3-location enhanced model resolver as offline diarization (`app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models`), and failures SHALL report all searched locations.

#### Scenario: Streaming diarization initialization failure
- **WHEN** the polyvoice `StreamingPipeline` fails to initialize in Fast mode because the bundled enhanced model files are absent or corrupt in all fallback locations
- **THEN** the system SHALL log the error with all searched directories, emit a warning event to the frontend noting that the enhanced models are bundled at build time near the executable, and continue recording without diarization; offline diarization remains available after recording stops (but also fails if the enhanced models are missing in all locations)

#### Scenario: Embedding extraction runtime error
- **WHEN** the enhanced embedding model fails during recording in Efficient mode
- **THEN** the system SHALL log the error, clear the embedding buffer, and allow offline diarization to run after recording stops

### Requirement: Online diarization resolves enhanced models from install locations
The system SHALL resolve the enhanced model directory for online diarization (both Efficient and Fast modes) via the same 3-location fallback chain as offline diarization. `OnlineDiarizationProcessor::new` SHALL accept an `AppHandle` (or a resolved `PathBuf` from the shared resolver) instead of a raw `models_dir` that assumes AppData only. When the enhanced files are present in `resource_dir/models` but not in AppData, online diarization SHALL initialize from the bundled resources without copying.

#### Scenario: Online init succeeds from bundled resources
- **WHEN** a recording starts in Fast or Efficient mode and `app_data_dir/models` is empty but `resource_dir/models` contains verified `segmentation-3.0.onnx` + `titanet_large.onnx`
- **THEN** `OnlineDiarizationProcessor::new` SHALL succeed using the resource location and online diarization SHALL be active for the session

#### Scenario: Online init fails lists all locations
- **WHEN** a recording starts with online diarization enabled but no fallback location contains verified enhanced files
- **THEN** initialization SHALL fail with an error listing all three searched directories and the build-time bundling explanation, and recording SHALL continue without online diarization

#### Scenario: Online and offline share resolver
- **WHEN** `check_diarization_models` reports `ready=true`
- **THEN** a subsequent `OnlineDiarizationProcessor::new` SHALL succeed using the same resolved directory

### Requirement: Multiple recordings cannot run online diarization simultaneously
The system SHALL enforce that at most one online diarization session is active at a time, reusing the existing `DiarizationGuard` pattern.

#### Scenario: Second recording blocked from online diarization
- **WHEN** a recording is already in progress with online diarization active and a second recording attempts to start with online diarization enabled
- **THEN** the second recording SHALL proceed without online diarization (transcription-only), and the frontend SHALL be notified that online diarization is unavailable

### Requirement: Efficient mode clusters with the calibrated threshold

The system SHALL cluster the buffered speaker embeddings at recording stop in Efficient mode using the same fixed cosine-similarity threshold as the enhanced TitaNet-Large offline family, so distinct speakers are assigned distinct labels and online/offline results stay consistent.

#### Scenario: Multi-speaker recording produces distinct labels

- **WHEN** recording stops in Efficient mode with buffered embeddings containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and matched transcripts SHALL be assigned distinct speaker IDs

#### Scenario: Clustering matches offline label scheme

- **WHEN** an online-Efficient-diarized meeting is re-analyzed with offline diarization
- **THEN** both paths SHALL apply the same enhanced family threshold, producing the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and comparable speaker counts

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
In Fast mode, the system SHALL run its own enhanced TitaNet-Large embedding extraction (192-d, layout-correct) on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does. The buffered embeddings SHALL be attributable to a specific pipeline speaker identity and channel so they can be grouped for enrollment without mixing channels.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks, each 192-d and produced with the TitaNet-correct layout

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

### Requirement: Stop-time assignment uses token-level refinement
At recording stop, when buffered transcript segments carry token timestamps, speaker matching SHALL refine ownership at token granularity before writing labels: tokens SHALL be attributed to the speaker of the covering turn, and a transcript segment spanning a speaker change SHALL be split into separate transcript rows at the boundary token. Segment overlap matching SHALL be used for segments without token timestamps. Results SHALL be written through the same `update_transcript_speaker` path as offline diarization.

#### Scenario: Cross-speaker chunk split at stop
- **WHEN** recording stops and a buffered transcript chunk with token timestamps spans `N≥2` distinct speakers detected by the online diarization turns
- **THEN** the chunk SHALL be stored as `N` transcript rows, one per contiguous speaker block (each boundary requires ≥2 contiguous tokens of the new speaker), each labeled with its block's speaker and timestamps adjusted to that block's token span, via the same transcript write path used for segment-level matching

#### Scenario: Single-speaker chunk labeled normally
- **WHEN** recording stops and a buffered transcript chunk with token timestamps is covered by a single speaker's turns
- **THEN** the chunk SHALL be labeled with that speaker without splitting

#### Scenario: No token timestamps falls back to overlap
- **WHEN** recording stops and a buffered transcript chunk has no token timestamps
- **THEN** the chunk SHALL be labeled by maximum temporal overlap with the diarization turns, exactly as before this change

### Requirement: Online TitaNet embedding layout correctness

Online diarization (both Efficient and Fast modes) SHALL extract TitaNet-Large (192-d, `titanet_large`) embeddings using the same mel-as-dim-1 layout fix applied offline. Any per-chunk embedding buffered during recording and any stop-time embedding derived from those chunks SHALL be produced with the TitaNet-correct tensor layout, so recording-stop clustering does not replay the offline shape mismatch.

#### Scenario: Efficient mode buffers valid TitaNet embeddings

- **WHEN** a recording in Efficient mode captures several VAD speech chunks of varied length
- **THEN** each chunk's embedding SHALL be produced without `Got invalid dimensions for input: audio_signal Expected:80` errors, and the buffered set SHALL be 192-d vectors usable for `AhcClusterer`

#### Scenario: Fast mode stop-time grouping uses layout-correct embeddings

- **WHEN** a Fast-mode recording stops and stop-time grouping derives per-cluster embeddings from the buffered per-chunk embeddings (grouped by pipeline speaker identity and channel)
- **THEN** the derived embeddings SHALL be TitaNet-correct and SHALL not inflate clustering with layout-corrupted vectors

#### Scenario: Online batch and single-chunk paths agree

- **WHEN** online diarization embeds a batch of chunks versus one-by-one on the same audio
- **THEN** both paths SHALL produce order-preserving 192-d outputs with the same layout, matching the offline contract

### Requirement: Online diarization fails loudly when no valid embeddings buffered

If online embedding produces zero valid embeddings for a channel (every chunk failed), that channel's stop-time clustering SHALL skip that channel distinctly and the overall diarization SHALL NOT report a silent `0 speakers` success for that channel without a warning log. When both channels have zero valid embeddings and at least one chunk existed, the run SHALL allow offline diarization to be the recovery path and SHALL log the layout/underlying error context.

#### Scenario: No valid embeddings on a channel skips the channel

- **WHEN** Efficient or Fast mode buffered chunks for the microphone channel but every embedding for that channel failed
- **THEN** stop-time processing SHALL log a warning with the underlying error detail, SHALL NOT produce empty clusters for that channel, and SHALL still process the other channel's valid embeddings normally

