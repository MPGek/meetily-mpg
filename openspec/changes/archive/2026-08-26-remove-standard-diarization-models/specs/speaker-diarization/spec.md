# Delta spec: speaker-diarization

## ADDED Requirements

### Requirement: Diarization requires the enhanced model set
The system SHALL run speaker diarization exclusively on the enhanced model set (pyannote `segmentation-3.0` segmentation + NVIDIA TitaNet-Large embedding, bundled at build time). When either enhanced file is missing or corrupt, diarization SHALL fail with a clear, actionable error rather than falling back to any other model set.

#### Scenario: Enhanced models missing fails offline diarization
- **WHEN** offline diarization is triggered but the bundled enhanced model files are absent or corrupt
- **THEN** the system SHALL return an error explaining that the enhanced diarization models are required (and are bundled at build time), and SHALL NOT attempt segmentation or embedding with any standard/legacy model

#### Scenario: Enhanced models missing disables online diarization
- **WHEN** online diarization (Fast or Efficient mode) is active for a recording but the bundled enhanced model files are absent or corrupt
- **THEN** the system SHALL log the error, notify the frontend that the enhanced models are required, and SHALL NOT run diarization with a fallback model set

### Requirement: Diarization model availability
The system SHALL report the availability of the bundled enhanced diarization model set (pyannote `segmentation-3.0` + TitaNet-Large) through the settings interface. The enhanced models SHALL be bundled at build time and verified against known file sizes; there SHALL be no runtime download or removal capability. The system SHALL remove stale model files from the model directory when model status is checked.

#### Scenario: Enhanced model status reported read-only
- **WHEN** user opens the diarization settings section
- **THEN** the UI SHALL show read-only verification status for the bundled enhanced segmentation and embedding files, with no download, re-download, or remove controls

#### Scenario: Model download control removed
- **WHEN** user opens the diarization settings section
- **THEN** no "Download Models", "Re-download Models", or remove control SHALL be shown, and no `download_diarization_models` runtime flow SHALL exist to invoke

#### Scenario: Missing or corrupt enhanced model detected
- **WHEN** a bundled enhanced model file is missing or fails verification
- **THEN** the system SHALL report the model set as unavailable in the settings panel, and diarization SHALL fail with a clear message (no fallback to a standard model set)

#### Scenario: Stale model files cleaned up
- **WHEN** model status is checked and stale model files are present in the model directory (sherpa-era `pyannote`/`3dspeaker` files, or standard polyvoice `powerset_int8.onnx` / `resnet34_int8.onnx`)
- **THEN** the system SHALL remove the stale files so only the enhanced model files remain

## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using the enhanced polyvoice ONNX model set (pyannote `segmentation-3.0` segmentation, TitaNet-Large speaker embedding, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio
- **THEN** the system runs segmentation, embedding extraction, and clustering, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use the same enhanced model set (segmentation-3.0, TitaNet-Large, AHC clustering) with no sherpa-onnx code or models in any diarization path

#### Scenario: Embedding extraction uses multiple cores
- **WHEN** offline diarization runs on a meeting with more than one detected speech segment
- **THEN** the system SHALL extract speaker embeddings using a batched, multi-session ONNX inference path that utilizes more than one CPU core

#### Scenario: Stereo channels diarize in parallel
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL run the microphone-channel and system-channel diarization passes concurrently, not sequentially

#### Scenario: Long recordings process without unbounded memory growth
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

### Requirement: Clustering distinguishes distinct speakers

The system SHALL cluster speaker embeddings with a fixed cosine-similarity threshold calibrated to the enhanced TitaNet-Large model family, so that distinct speakers in the audio are assigned distinct speaker labels rather than being merged into a single cluster.

#### Scenario: Multi-speaker meeting produces distinct labels

- **WHEN** offline diarization runs on a recording containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and the matched transcripts SHALL be assigned distinct speaker IDs (e.g. `SPEAKER_00` and `SPEAKER_01`)

#### Scenario: Single-speaker recording stays single-labeled

- **WHEN** offline diarization runs on a recording containing a single speaker on a channel
- **THEN** all matched transcripts SHALL be assigned the same speaker ID rather than being split into multiple spurious labels

## REMOVED Requirements

### Requirement: Diarization model management
**Reason**: The runtime download flow for the standard polyvoice model set (`powerset_int8` + `resnet34_int8`) is removed; diarization now runs exclusively on the enhanced model set bundled at build time, so there is nothing to download, progress-report, or re-download.

**Migration**: The `check_diarization_models` command returns the read-only enhanced availability status under "Diarization model availability". The `download_diarization_models`, `download_enhanced_diarization_models`, and `remove_enhanced_diarization_models` commands and their frontend listeners are deleted; the settings "Download Models" control is removed. Users on builds without the bundled enhanced set see the read-only unavailable state.