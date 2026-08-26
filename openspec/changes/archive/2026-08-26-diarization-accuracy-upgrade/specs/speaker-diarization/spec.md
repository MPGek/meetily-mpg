## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using ONNX-based diarization models that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL be model-aware: when the enhanced model set (pyannote `segmentation-3.0` segmentation and TitaNet-Large embeddings) is installed, the pipeline SHALL run segmentation, embedding extraction, and clustering against that set; otherwise it SHALL run against the bundled polyvoice model set (powerset segmentation, speaker embedding extraction, and agglomerative clustering). Both sets SHALL share the same pipeline mechanics: multiple CPU cores during embedding extraction, batched embedding inference, concurrent stereo channel processing, bounded memory for long recordings, and AHC clustering.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio
- **THEN** the system runs segmentation, embedding extraction, and clustering, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization uses enhanced models when installed
- **WHEN** the enhanced model set is installed and diarization is triggered
- **THEN** segmentation SHALL run with the `segmentation-3.0` model and embeddings SHALL be extracted with the TitaNet-Large model, while chunking, batched embedding, per-channel parallelism, and AHC clustering behavior remain the same as the legacy path

#### Scenario: Fallback to bundled polyvoice models
- **WHEN** the enhanced model set is not installed and diarization is triggered
- **THEN** the pipeline SHALL run with the bundled polyvoice powerset segmentation and ResNet34 embedding models, producing results equivalent to current behavior

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
The system SHALL support speaker diarization ONNX models. The legacy polyvoice models SHALL be obtained and verified via the polyvoice ModelRegistry (SHA-256 checksum and minisign signature) in the app's model directory, exactly as today. The enhanced model set (`onnx-community/pyannote-segmentation-3.0` and `Recogment/titanet-large-onnx` TitaNet-Large, both public) SHALL be fetched **at build time** by `build.rs` / `scripts/fetch-enhanced-models.*` (no `HF_TOKEN`, no `dotenv`), verified with repository-managed SHA-256 checksum and signature, and bundled as app resources; **no runtime downloading**. At runtime the system SHALL only verify bundled files.

#### Scenario: Download diarization models
- **WHEN** user clicks "Download Models" in the diarization settings section
- **THEN** the system downloads the powerset segmentation model `powerset_int8` (~1.6MB) and the speaker embedding model `resnet34_int8` (~6.8MB) to the app's model directory via the polyvoice ModelRegistry, verifying checksums and signatures

#### Scenario: Download enhanced diarization models
- **WHEN** building (no `HF_TOKEN` required)
- **THEN** the build script SHALL download the enhanced segmentation model (`segmentation-3.0` ONNX from `onnx-community/pyannote-segmentation-3.0`, public) and the enhanced embedding model (`titanet-large.onnx` from `Recogment/titanet-large-onnx`, public, ~97 MB) to the bundled resources, verifying the repository-managed SHA-256 checksum and signature, and SHALL mark the enhanced set as available only after both files verify (no runtime download, both files required)

#### Scenario: Enhanced set not bundled when build offline
- **WHEN** building without network (offline) or a public fetch fails
- **THEN** the build SHALL skip with a warning and SHALL NOT bundle the enhanced set; the runtime SHALL fall back to the legacy polyvoice set

#### Scenario: Enhanced set availability shown in settings
- **WHEN** the user opens the diarization settings
- **THEN** the panel SHALL show read-only status of both model sets (no download/remove controls), indicating whether the enhanced set is bundled and therefore used by default

#### Scenario: Model download progress reporting
- **WHEN** enhanced models are being fetched at build time
- **THEN** the build script SHALL log per-model progress; at runtime the system SHALL NOT emit download progress events (verification is read-only of bundled files, legacy download progress via ModelRegistry remains for the bundled polyvoice pair)

#### Scenario: Re-download models
- **WHEN** rebuilding and bundled files already exist
- **THEN** the build script SHALL re-fetch and overwrite the bundled files (no confirmation prompt); there is no runtime re-download or confirmation dialog

#### Scenario: Legacy model files cleaned up
- **WHEN** model checking or download runs and stale sherpa-era files (pyannote `model.int8.onnx`, 3D-Speaker `3dspeaker_*.onnx`) are present in the model directory
- **THEN** the system SHALL remove the stale files so only polyvoice registry and enhanced-set models remain

#### Scenario: Missing or corrupt model detected
- **WHEN** a required model file is missing or fails its checksum/signature verification
- **THEN** the system SHALL report the model as unavailable in the settings panel and request a re-download before that model set can be used; when the missing set is the enhanced one, the pipeline SHALL fall back to the legacy set rather than failing

### Requirement: Clustering distinguishes distinct speakers

The system SHALL cluster speaker embeddings with a per-model-family cosine-similarity threshold: 0.45 for the bundled polyvoice (ResNet34) family, and a separately calibrated threshold constant for the enhanced (TitaNet) family. The threshold SHALL be consistent between offline and online paths for the same model family, so that distinct speakers in the audio are assigned distinct speaker labels rather than being merged into a single cluster.

#### Scenario: Multi-speaker meeting produces distinct labels

- **WHEN** offline diarization runs on a recording containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and the matched transcripts SHALL be assigned distinct speaker IDs (e.g. `SPEAKER_00` and `SPEAKER_01`)

#### Scenario: Single-speaker recording stays single-labeled

- **WHEN** offline diarization runs on a recording containing a single speaker on a channel
- **THEN** all matched transcripts SHALL be assigned the same speaker ID rather than being split into multiple spurious labels

#### Scenario: Enhanced family uses its own threshold

- **WHEN** clustering runs with embeddings produced by the enhanced TitaNet model
- **THEN** the system SHALL apply the calibrated TitaNet threshold for cluster merging, not the ResNet34 0.45 threshold

### Requirement: Speaker label assignment

The system SHALL assign speaker labels to transcript segments by matching diarization time ranges to transcript timestamps, filling short gaps with the nearest speaker. When the ASR engine provides token-level timestamps for a transcript segment, the system SHALL refine the assignment at token granularity: tokens SHALL be attributed to the speaker whose diarization turn covers each token, and a transcript segment whose tokens span a speaker change SHALL be split into separate transcript rows at the boundary token, each labeled with its own speaker. Segment-level overlap matching SHALL remain the fallback when token timestamps are unavailable.

#### Scenario: Overlap-based speaker matching

- **WHEN** diarization produces speaker turns with start/end times and no token timestamps are available for a segment
- **THEN** each transcript segment SHALL be assigned the speaker whose time range has the maximum overlap with the segment's `audio_start_time` to `audio_end_time`

#### Scenario: Token-level assignment splits a cross-speaker segment

- **WHEN** a transcript segment has token timestamps and its tokens span `N≥2` distinct speakers (e.g. A→B→A or A→B→C mid-utterance)
- **THEN** the segment SHALL be split into `N` transcript rows, one per contiguous speaker block (each boundary requires ≥2 contiguous tokens of the new speaker), each row labeled with its block's speaker and `audio_start_time`/`audio_end_time` adjusted to that block's token span; `source_device` and other columns SHALL be preserved per row with contiguous, gap-free ordering

#### Scenario: Token-level assignment within one speaker

- **WHEN** a transcript segment has token timestamps and all its tokens fall within turns of a single speaker (or turns of the same speaker with short gaps)
- **THEN** the segment SHALL keep a single speaker label from token assignment, matching the speaker of the covering turns

#### Scenario: Unmatched transcript segments

- **WHEN** a transcript segment has no overlapping diarization turn and cannot be gap-filled (the channel has no turns, or the nearest turn on a multi-speaker channel is beyond 30 seconds)
- **THEN** the segment's `speaker` SHALL remain NULL

#### Scenario: Gap-fill on single-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel's turns all belong to a single speaker
- **THEN** the segment SHALL be assigned that channel's single speaker

#### Scenario: Gap-fill bounded on multi-speaker channel

- **WHEN** a transcript segment has no overlapping diarization turn and the channel has multiple speakers
- **THEN** the segment SHALL be assigned the temporally nearest turn's speaker when that turn lies within 30 seconds, and SHALL remain NULL otherwise

## ADDED Requirements

### Requirement: Enhanced diarization models default when installed
When the enhanced model set is installed, offline diarization SHALL use it by default for new and re-run diarization; the legacy polyvoice set SHALL remain bundled and serve as the automatic fallback. Diarization SHALL never require the user to have the enhanced models installed.

#### Scenario: Enhanced models used once bundled at build
- **WHEN** diarization runs after the enhanced model set has been bundled at build time
- **THEN** the pipeline SHALL use the enhanced segmentation and embedding models without further configuration, and the results SHALL be tagged with the enhanced model family so recognition, caching, and enrollment treat their embeddings correctly

#### Scenario: Legacy models until bundled
- **WHEN** diarization runs when the enhanced model set is not bundled
- **THEN** the pipeline SHALL use the bundled polyvoice models and SHALL produce the same behavior as before this change

#### Scenario: Re-run after enhanced models not bundled (rebuild without)
- **WHEN** the enhanced models are not bundled (rebuild without them) and diarization re-runs on a previously enhanced-analyzed meeting
- **THEN** the pipeline SHALL fall back to the legacy models and overwrite transcript speaker labels with legacy-family results, consistent with current re-analysis behavior