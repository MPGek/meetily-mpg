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
- **WHEN** offline diarization runs on a recording of any length
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
The system SHALL process the microphone (left) and system (right) channels of a stereo recording independently during offline diarization, splitting the audio into two mono streams before segmentation.

#### Scenario: Stereo recording diarized per channel
- **WHEN** offline diarization runs on a stereo recording (2 channels, left=microphone, right=system)
- **THEN** the system SHALL produce a 16kHz microphone stream and a 16kHz system stream, resample each to 16kHz independently, and run the diarization pipeline once per channel, reusing a single diarizer instance loaded once

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
The system SHALL allow users to assign human-readable names to speaker clusters. The speaker editor SHALL default to single-block scope (a per-transcript override that relabels only the edited block); an explicit "apply to all blocks of this speaker" option SHALL link the whole cluster to a registry speaker (created on demand for new names) via the `meeting_speakers` mapping with `matched_by='user'`, and SHALL enroll the cluster's cached embeddings as that speaker's prototypes. Renaming a linked registry speaker SHALL apply globally across all meetings; display names SHALL be resolved at read time by joining `meeting_speakers` to `speakers`, with per-block overrides taking precedence and legacy `transcripts.speaker_label` as fallback.

#### Scenario: Rename speaker inline
- **WHEN** user edits a speaker label in the transcript view (e.g., changes "SPEAKER_00" to "Alice") using the default single-block scope
- **THEN** only that transcript block SHALL display "Alice"; the cluster's `meeting_speakers` mapping SHALL remain unchanged and the block's cached embeddings SHALL NOT be enrolled

#### Scenario: Name whole cluster
- **WHEN** user edits a speaker label in the transcript view with the "apply to all blocks of this speaker" option (e.g., changes "SPEAKER_00" to "Alice")
- **THEN** the system SHALL upsert the cluster's `meeting_speakers` row to link the registry speaker "Alice" (creating her if new) with `matched_by='user'`, enroll the cluster's cached embeddings to Alice, and display "Alice" on all transcripts with that cluster label

#### Scenario: Speaker label appears in UI
- **WHEN** a transcript segment's cluster is linked to a registry speaker (or has a legacy `speaker_label`)
- **THEN** the UI SHALL display the resolved name instead of the raw speaker ID

#### Scenario: Global rename
- **WHEN** user renames a linked speaker from "Alice" to "Alice Smith"
- **THEN** the name "Alice Smith" SHALL appear for that speaker in every meeting where she is linked (including any per-block overrides referencing her)

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

### Requirement: Streaming audio decode via ffmpeg
The system SHALL decode, resample, and channel-split recorded audio for offline diarization by streaming it through a spawned ffmpeg process on stdout, rather than decoding the entire file into memory. The system SHALL consume the stream in bounded in-memory chunks so peak memory does not scale with recording length, and SHALL fall back to the existing Symphonia decoder only when ffmpeg is unavailable.

#### Scenario: Stereo recording decoded and split via ffmpeg
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL spawn ffmpeg to decode the file, resample each channel to 16kHz, and emit the microphone (left) and system (right) channels as separate mono streams, so the Rust pipeline receives per-channel 16kHz samples without ever holding the full decoded interleaved buffer

#### Scenario: Decode memory is bounded
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL hold only a single in-memory audio chunk per channel at a time during segmentation and embedding, and SHALL NOT materialize a full-length decoded sample buffer

#### Scenario: ffmpeg unavailable falls back to Symphonia
- **WHEN** the ffmpeg executable cannot be located
- **THEN** the system SHALL decode the audio with the existing Symphonia path and continue diarization, at the cost of higher peak memory

### Requirement: Fixed diarization concurrency profile
The system SHALL use a single fixed concurrency and memory profile for offline diarization with no user-facing memory-mode or session-count settings. The system SHALL size the segmenter and embedder ONNX session pools to the smaller of 8 or 75% of the logical CPU core count (rounded up, minimum 1), and SHALL always process recordings in overlapping chunks.

#### Scenario: Session pool sized from core count
- **WHEN** offline diarization runs on a machine with N logical CPU cores
- **THEN** the segmenter and embedder session pools SHALL each contain `min(8, ceil(0.75 × N))` sessions, never exceeding 8

#### Scenario: Single-core floor
- **WHEN** offline diarization runs on a machine with one logical CPU core
- **THEN** the session pools SHALL each contain exactly 1 session

#### Scenario: No memory-mode setting
- **WHEN** the user opens the diarization settings
- **THEN** the system SHALL NOT present a memory-mode selector or a session-count override, and diarization SHALL always use the fixed profile

### Requirement: Chunked offline diarization
The system SHALL process recordings in overlapping temporal chunks to bound memory usage, using a fixed chunk duration.

#### Scenario: Chunking triggered by duration
- **WHEN** a channel's duration exceeds the fixed chunk duration
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

### Requirement: Cluster embedding cache persistence
Offline diarization SHALL persist, for each produced cluster, its centroid embedding and a bounded set of exemplar embeddings (with per-segment durations) as unassigned cache rows owned by `(meeting_id, cluster_label)`, in addition to writing transcript cluster labels.

#### Scenario: Cache survives diarization
- **WHEN** offline diarization completes for a meeting
- **THEN** each produced cluster SHALL have a stored centroid in `meeting_speakers` and exemplar embeddings in `speaker_embeddings` keyed by `(meeting_id, cluster_label)`

### Requirement: Post-clustering automatic recognition
After clustering and cache persistence, offline diarization SHALL match each cluster centroid against candidate prototypes per the speaker-identity-registry recognition rules (expected-speaker allowlist, or all registry speakers when no allowlist; model-tagged embeddings only; τ=0.7) and auto-assign confident matches.

#### Scenario: Recognized speaker labeled without user action
- **WHEN** offline diarization completes and a cluster centroid matches an expected speaker's prototype above threshold
- **THEN** the system SHALL set the cluster's `meeting_speakers.speaker_id` with `matched_by='auto'` and the match score, and the meeting's transcripts SHALL display the speaker's name

#### Scenario: No candidates leaves clusters anonymous
- **WHEN** the registry is empty or no candidate exceeds the threshold
- **THEN** diarization results SHALL be unchanged from current behavior (cluster labels only)

