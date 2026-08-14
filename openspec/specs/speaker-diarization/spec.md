# speaker-diarization Specification

## Purpose
Speaker identification ("who spoke when") using ONNX-based diarization models running locally as a post-processing step on recorded meetings.
## Requirements
### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using polyvoice ONNX models (powerset segmentation, speaker embedding extraction, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio
- **THEN** the system runs segmentation, embedding extraction, and clustering, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use polyvoice components (streaming pipeline with a polyvoice ONNX embedder and AHC clustering) with no sherpa-onnx code or models in any diarization path

#### Scenario: Embedding extraction uses multiple cores
- **WHEN** offline diarization runs on a meeting with more than one detected speech segment
- **THEN** the system SHALL extract speaker embeddings using a batched, multi-session ONNX inference path that utilizes more than one CPU core

#### Scenario: Stereo channels diarize in parallel
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL run the microphone-channel and system-channel diarization passes concurrently, not sequentially

#### Scenario: Long recordings process without unbounded memory growth
- **WHEN** offline diarization runs on a recording longer than the configured chunk threshold
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

### Requirement: Diarization model management
The system SHALL support downloading and configuring speaker diarization ONNX models through the settings interface, with model files obtained and verified via the polyvoice ModelRegistry (SHA-256 checksum and minisign signature) in the app's model directory.

#### Scenario: Download diarization models
- **WHEN** user clicks "Download Models" in the diarization settings section
- **THEN** the system downloads the powerset segmentation model `powerset_int8` (~1.6MB) and the speaker embedding model `resnet34_int8` (~6.8MB) to the app's model directory via the polyvoice ModelRegistry, verifying checksums and signatures

#### Scenario: Model download progress reporting
- **WHEN** models are being downloaded
- **THEN** the system SHALL emit progress events per model with the model name and an overall percentage across the two models

#### Scenario: Re-download models
- **WHEN** user clicks download and models already exist on disk
- **THEN** the system SHALL re-download and overwrite existing files, showing a confirmation prompt first

#### Scenario: Legacy model files cleaned up
- **WHEN** model checking or download runs and stale sherpa-era files (pyannote `model.int8.onnx`, 3D-Speaker `3dspeaker_*.onnx`) are present in the model directory
- **THEN** the system SHALL remove the stale files so only polyvoice registry models remain

#### Scenario: Missing or corrupt model detected
- **WHEN** a required model file is missing or fails its checksum/signature verification
- **THEN** the system SHALL report the model as unavailable in the settings panel and request a re-download before diarization can run

### Requirement: Diarization trigger modes
The system SHALL support automatic diarization after recording stops (when enabled) and manual diarization on any past meeting.

#### Scenario: Auto-trigger after recording stops
- **WHEN** diarization is enabled in settings and recording stops
- **THEN** the system SHALL automatically start diarization on the saved recording

#### Scenario: Manual trigger on past meeting
- **WHEN** user clicks "Re-analyze Speakers" on a meeting detail page
- **THEN** the system SHALL start diarization on that meeting's audio, re-processing even if previously diarized

#### Scenario: Diarization disabled — no auto-trigger
- **WHEN** diarization is disabled in settings
- **THEN** no diarization SHALL run automatically after recording stops

### Requirement: Diarization progress reporting
The system SHALL emit progress events during diarization so the frontend can display status.

#### Scenario: Progress updates during processing
- **WHEN** diarization is running
- **THEN** the system SHALL emit `diarization-progress` events with `status`, `progress` (0-100), and `message` fields

#### Scenario: Diarization completion
- **WHEN** diarization finishes successfully
- **THEN** the system SHALL emit a final progress event with `status: "complete"` and `progress: 100`

#### Scenario: Diarization failure
- **WHEN** diarization encounters an unrecoverable error
- **THEN** the system SHALL emit a progress event with `status: "failed"` and an error message, and set `diarization_status` to "failed" on the meeting

### Requirement: Speaker label assignment

The system SHALL assign speaker labels to transcript segments by matching diarization time ranges to transcript timestamps, filling short gaps with the nearest speaker.

#### Scenario: Overlap-based speaker matching

- **WHEN** diarization produces speaker turns with start/end times
- **THEN** each transcript segment SHALL be assigned the speaker whose time range has the maximum overlap with the segment's `audio_start_time` to `audio_end_time`

#### Scenario: Unmatched transcript segments

- **WHEN** a transcript segment has no overlapping diarization turn and cannot be gap-filled (the channel has no turns, or the nearest turn on a multi-speaker channel is beyond 30 seconds)
- **THEN** the segment's `speaker` SHALL remain NULL

#### Scenario: Gap-fill on single-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel's turns all belong to a single speaker
- **THEN** the segment SHALL be assigned that channel's single speaker

#### Scenario: Gap-fill bounded on multi-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel has multiple speakers
- **THEN** the segment SHALL be assigned the temporally nearest turn's speaker when that turn lies within 30 seconds, and SHALL remain NULL otherwise

### Requirement: System audio transcripts diarized from system channel
The system SHALL assign speaker labels to system-source transcripts by diarizing the system channel independently, instead of overriding them with a single "SystemAudio" label.

#### Scenario: System transcripts get remote speaker IDs
- **WHEN** a transcript segment has `source_device="System"` and the system-channel diarization run produces a speaker turn overlapping its time range
- **THEN** the segment's `speaker` SHALL be set to the matching `SPEAKER_NN` ID

#### Scenario: System transcripts without a match
- **WHEN** a transcript segment has `source_device="System"` and no system-channel speaker turn overlaps its time range
- **THEN** the segment's `speaker` SHALL remain NULL

### Requirement: Per-channel offline diarization
The system SHALL process the microphone (left) and system (right) channels of a stereo recording independently during offline diarization, de-interleaving the decoded audio into two mono streams before segmentation.

#### Scenario: Stereo recording diarized per channel
- **WHEN** offline diarization runs on a stereo recording (2 channels, left=microphone, right=system)
- **THEN** the system SHALL de-interleave the decoded samples into a microphone stream and a system stream, resample each to 16kHz independently, and run the diarization pipeline once per channel, reusing a single diarizer instance loaded once

#### Scenario: Silent channel produces no speakers
- **WHEN** one channel of a stereo recording contains no speech
- **THEN** the system SHALL produce no speaker segments for that channel and its transcripts SHALL remain unlabeled rather than failing the whole run

### Requirement: Channel-specific speaker IDs
The system SHALL namespace speaker IDs by source channel so that cluster indices from the two independent diarization runs do not collide.

#### Scenario: Microphone speakers named
- **WHEN** the microphone-channel diarization run produces clusters 0..N
- **THEN** transcripts matched to those segments SHALL be assigned speaker IDs `MIC_SPEAKER_00` through `MIC_SPEAKER_NN`

#### Scenario: System speakers named
- **WHEN** the system-channel diarization run produces clusters 0..N
- **THEN** transcripts matched to those segments SHALL be assigned speaker IDs `SPEAKER_00` through `SPEAKER_NN`

#### Scenario: Per-source segment matching
- **WHEN** matching diarization segments to transcript time ranges
- **THEN** transcripts with `source_device="Microphone"` (or NULL) SHALL be matched against microphone-channel segments, and transcripts with `source_device="System"` SHALL be matched against system-channel segments

### Requirement: Mono recording fallback
The system SHALL treat mono recordings (or files without a distinct system channel) as a single remote-only source during offline diarization.

#### Scenario: Mono recording diarized as remote
- **WHEN** offline diarization runs on a mono recording
- **THEN** the system SHALL run the diarization pipeline once on the mono stream and assign all matched transcripts `SPEAKER_NN` IDs regardless of `source_device`

### Requirement: Prefixed speaker ID rendering
The transcript view SHALL render both speaker ID namespaces correctly and degrade gracefully for legacy labels.

#### Scenario: Mic speaker label display
- **WHEN** a transcript segment has speaker `MIC_SPEAKER_03`
- **THEN** the UI SHALL display "Mic Speaker 4" using the speaker color for index 3

#### Scenario: Remote speaker label display
- **WHEN** a transcript segment has speaker `SPEAKER_00`
- **THEN** the UI SHALL display "Speaker 1" using the speaker color for index 0

#### Scenario: Legacy SystemAudio label display
- **WHEN** a transcript segment retains the legacy speaker value `SystemAudio` from a prior diarization run
- **THEN** the UI SHALL display "System Audio" with a stable color instead of "Speaker NaN"

### Requirement: Speaker naming and labels
The system SHALL allow users to assign human-readable names to speaker IDs on a per-meeting basis.

#### Scenario: Rename speaker inline
- **WHEN** user edits a speaker label in the transcript view (e.g., changes "SPEAKER_00" to "Alice")
- **THEN** the system SHALL persist the label to the `speaker_label` column on all transcripts with that speaker ID, and update `speaker_names` JSON on the meeting record

#### Scenario: Speaker label appears in UI
- **WHEN** a transcript segment has a `speaker_label` value
- **THEN** the UI SHALL display the label instead of the raw speaker ID

### Requirement: Diarization cancellation
The system SHALL support cancelling an in-progress diarization job.

#### Scenario: Cancel during processing
- **WHEN** user triggers a new diarization or closes the app while diarization is running
- **THEN** the system SHALL cancel the current diarization task gracefully, leaving partial results (if any) in place

### Requirement: Diarization configuration
The system SHALL persist diarization configuration in user settings.

#### Scenario: Save diarization preferences
- **WHEN** user changes diarization settings (enabled/disabled, max speakers, auto-run)
- **THEN** the system SHALL persist these to the settings store

### Requirement: Clustering distinguishes distinct speakers

The system SHALL cluster speaker embeddings with a fixed cosine-similarity threshold calibrated to the Balanced profile (`0.45`), so that distinct speakers in the audio are assigned distinct speaker labels rather than being merged into a single cluster.

#### Scenario: Multi-speaker meeting produces distinct labels

- **WHEN** offline diarization runs on a recording containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and the matched transcripts SHALL be assigned distinct speaker IDs (e.g. `SPEAKER_00` and `SPEAKER_01`)

#### Scenario: Single-speaker recording stays single-labeled

- **WHEN** offline diarization runs on a recording containing a single speaker on a channel
- **THEN** all matched transcripts SHALL be assigned the same speaker ID rather than being split into multiple spurious labels

### Requirement: Max speakers setting caps cluster count

The system SHALL apply the user-configured `maxSpeakers` setting as a hard ceiling on the number of clusters produced during offline diarization, and SHALL use no ceiling when the setting is unset or zero.

#### Scenario: Max speakers set

- **WHEN** offline diarization runs and the `maxSpeakers` setting is a positive value N
- **THEN** the clustering SHALL produce at most N distinct speaker labels

#### Scenario: Max speakers unset

- **WHEN** offline diarization runs and the `maxSpeakers` setting is unset or zero
- **THEN** the clustering SHALL run without a ceiling and infer the speaker count automatically

### Requirement: Singleton cluster pruning

The system SHALL dissolve single-segment clusters by reassigning their segments to the nearest larger speaker cluster, preventing spurious fragment speakers from inflating the speaker count.

#### Scenario: Singleton fragment reassigned

- **WHEN** clustering produces a cluster containing fewer than two segments
- **THEN** the system SHALL reassign that cluster's segments to the nearest cluster with at least two segments

### Requirement: Batch embedding extraction
The system SHALL extract speaker embeddings from all detected segments in a batch rather than one segment at a time.

#### Scenario: Batch embed call used
- **WHEN** the diarization pipeline has N detected speech segments after segmentation
- **THEN** the system SHALL invoke the embedder's batch interface with all N segment audio slices and receive N embeddings in one coordinated call

#### Scenario: Batch extraction preserves ordering
- **WHEN** embeddings are produced by the batch call
- **THEN** the i-th returned embedding SHALL correspond to the i-th input segment so that clustering and transcript matching remain correct

### Requirement: Configurable concurrency and memory mode
The system SHALL expose settings that control diarization concurrency and memory usage.

#### Scenario: Default auto mode
- **WHEN** diarization settings are at their default values
- **THEN** the system SHALL use an "auto" memory mode that balances speed and RAM, selecting a moderate ONNX session pool size and enabling chunking only for long recordings

#### Scenario: Fast mode maximizes concurrency
- **WHEN** the user selects the "fast" memory mode
- **THEN** the system SHALL use the maximum recommended number of ONNX sessions and disable chunked processing unless the recording exceeds a hard safety threshold

#### Scenario: Low-memory mode
- **WHEN** the user selects the "low memory" memory mode
- **THEN** the system SHALL limit the ONNX session pool to at most two sessions and always process recordings in chunks

#### Scenario: Custom max session count
- **WHEN** the user sets a specific maximum session count
- **THEN** the system SHALL cap the embedder and segmenter session pools at that value, overriding the auto-derived default

### Requirement: Chunked offline diarization
The system SHALL support processing long recordings in overlapping temporal chunks to bound memory usage.

#### Scenario: Chunking triggered by duration
- **WHEN** a channel's duration exceeds the configured chunk duration
- **THEN** the system SHALL split the channel into contiguous chunks with a fixed overlap, run segmentation and embedding on each chunk independently, and discard each chunk's audio before processing the next

#### Scenario: Overlap prevents boundary artifacts
- **WHEN** chunks are created
- **THEN** adjacent chunks SHALL overlap by at least 5 seconds so that speaker turns crossing a chunk boundary are captured in full within at least one chunk

#### Scenario: Global clustering across chunks
- **WHEN** all chunks have been processed and their embeddings accumulated
- **THEN** the system SHALL run a single clustering pass over the combined set of embeddings so that the same speaker receives the same label across chunk boundaries

### Requirement: Performance observability
The system SHALL log stage-level timing and peak memory information during offline diarization.

#### Scenario: Stage timing logged
- **WHEN** offline diarization completes
- **THEN** the system SHALL log elapsed time for decode, segmentation, embedding, clustering, and transcript matching stages

#### Scenario: Memory usage logged
- **WHEN** offline diarization completes
- **THEN** the system SHALL log peak working-set memory (or best-available proxy) and the final number of segments, embeddings, and speakers

#### Scenario: Regression warning
- **WHEN** a diarization run takes longer than a configurable threshold or exceeds a memory threshold
- **THEN** the system SHALL emit a warning log with the recorded metrics

