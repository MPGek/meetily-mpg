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
- **WHEN** the `StreamingPipeline` outputs a speaker turn during recording in Fast mode
- **THEN** the system SHALL buffer the turn internally (stable turns only) and NOT emit it to the frontend until recording stops

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

