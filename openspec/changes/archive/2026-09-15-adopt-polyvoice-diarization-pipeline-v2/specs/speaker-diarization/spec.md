## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using the enhanced polyvoice ONNX model set (pyannote `segmentation-3.0` segmentation with calibrated onset/offset hysteresis binarization, TitaNet-Large speaker embedding extracted over dense resegmentation windows, and automatic speaker-count clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL resolve its segmentation and embedding models through the 3-location fallback chain and SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings. Offline clustering SHALL run the resegmentation-based architecture (dense embedding windows, hysteresis binarization, overlap-aware two-speaker assignment, gap-fill) with the clusterer kind (automatic-count NME-SC or VBx, or fixed-threshold AHC) and its parameters resolved from the runtime-configurable parameter surface specified by the diarization-param-tuning capability, with built-in defaults chosen by the measured sweep protocol.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio and the enhanced models are present in any fallback location
- **THEN** the system runs segmentation, dense embedding extraction, resegmentation, and clustering via the resolved model directory, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use the same enhanced model set (segmentation-3.0, TitaNet-Large, AHC clustering) resolved via the shared 3-location fallback with no sherpa-onnx code or models in any diarization path

#### Scenario: Embedding extraction uses multiple cores
- **WHEN** offline diarization runs on a meeting with more than one detected speech segment
- **THEN** the system SHALL extract speaker embeddings using a batched, multi-session ONNX inference path that utilizes more than one CPU core

#### Scenario: Stereo channels diarize in parallel
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL run the microphone-channel and system-channel diarization passes concurrently, not sequentially

#### Scenario: Long recordings process without unbounded memory growth
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings, frame posteriors, and segment metadata between chunks, so peak memory does not grow linearly with recording duration

#### Scenario: Offline clustering respects the speaker-count ceiling
- **WHEN** offline diarization clusters a channel's embeddings
- **THEN** the number of distinct speaker labels in the result does not exceed the effective ceiling (user max-speakers when set, otherwise the configured default ceiling), and the clusterer kind, merge threshold, and gap-fill window come from the resolved runtime parameters

#### Scenario: Speaker count is inferred automatically
- **WHEN** offline diarization runs with an automatic-count clusterer kind (vbx or nmesc) and no user max-speakers is set
- **THEN** the pipeline SHALL select the number of speakers from the embedding data rather than a fixed similarity threshold, subject to the enforced ceiling

### Requirement: Diarization model management
The system SHALL support the enhanced diarization model set (pyannote `segmentation-3.0` + TitaNet-Large) resolving model files through a 3-location fallback chain — `app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models` (dev) — and verified by file existence and size (>1 KB; SHA-256 when published). There SHALL be no runtime download or removal capability. The VBx PLDA parameter files SHALL NOT be bundled: the vendored parameters require 256-dimensional embeddings while the enhanced TitaNet-Large family is 192-dimensional, so the `vbx` clusterer kind SHALL fail with an actionable error on this model family (no silent kind switch). The `check_diarization_models` command and the diarization engine SHALL use the same resolver so their readiness decisions never disagree. The system SHALL remove stale model files from the model directory when model status is checked.

#### Scenario: Download diarization models
- **WHEN** user opens the diarization settings section
- **THEN** the UI SHALL show read-only verification status for the bundled enhanced segmentation and embedding files (per-file ✓/○ + ready badge) with no "Download Models", "Re-download Models", or remove controls, and no `download_diarization_models` runtime flow SHALL exist to invoke

#### Scenario: Model download progress reporting
- **WHEN** the bundled enhanced model files are present in any of the three fallback locations (app_data, bundled resources, or dev manifest)
- **THEN** the system SHALL report `segmentation_ready=true` and `embedding_ready=true` (and `ready=true` only when both files verify in the same location) via the shared resolver

#### Scenario: Re-download models
- **WHEN** user inspects diarization settings and the bundled enhanced set is present in `resource_dir/models` but missing in `app_data_dir/models`
- **THEN** diarization SHALL still succeed by loading from the bundled resource location without requiring a copy or re-download

#### Scenario: Legacy model files cleaned up
- **WHEN** model checking runs and stale files are present in the app-data model directory (sherpa-era `pyannote`/`3dspeaker` files, or standard polyvoice `powerset_int8.onnx` / `resnet34_int8.onnx`)
- **THEN** the system SHALL remove the stale files so only the enhanced model files remain

#### Scenario: Missing or corrupt model detected
- **WHEN** a required enhanced model file is missing or fails verification in all three fallback locations
- **THEN** the system SHALL report the model as unavailable in the settings panel and diarization SHALL fail with a clear error listing all searched locations and noting that the models are bundled at build time near the executable

#### Scenario: VBx clusterer with the enhanced 192-d model family
- **WHEN** offline diarization runs with clusterer kind `vbx` on the enhanced TitaNet-Large model set (192-d embeddings, incompatible with the 256-d PLDA parameters)
- **THEN** the run SHALL fail with an actionable error naming the `vbx` embedding-dimension requirement, and SHALL NOT silently switch clusterers

#### Scenario: Automatic-count and AHC kinds without PLDA assets
- **WHEN** offline diarization runs with clusterer kind `ahc` or `nmesc` and no PLDA parameter files are present
- **THEN** diarization SHALL succeed normally, as the PLDA files are not required for those kinds

### Requirement: Clustering distinguishes distinct speakers

The system SHALL cluster speaker embeddings so that distinct speakers in the audio are assigned distinct speaker labels rather than being merged into a single cluster: with automatic-count kinds (vbx, nmesc) via speaker-count inference over the TitaNet-Large embedding set, and with the AHC kind via a fixed cosine-similarity threshold calibrated to the enhanced TitaNet-Large model family.

#### Scenario: Multi-speaker meeting produces distinct labels

- **WHEN** offline diarization runs on a recording containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and the matched transcripts SHALL be assigned distinct speaker IDs (e.g. `SPEAKER_00` and `SPEAKER_01`)

#### Scenario: Single-speaker recording stays single-labeled

- **WHEN** offline diarization runs on a recording containing a single speaker on a channel
- **THEN** all matched transcripts SHALL be assigned the same speaker ID rather than being split into multiple spurious labels

### Requirement: Max speakers setting caps cluster count

The system SHALL apply the user-configured `maxSpeakers` setting as a hard ceiling on the number of clusters produced during offline diarization when set, and SHALL apply the configured default speaker-count ceiling when the setting is unset or zero — the ceiling is always enforced for every clusterer kind (diarization-param-tuning). The effective ceiling SHALL be clamped to the clustering backend's supported maximum (255).

#### Scenario: Max speakers set

- **WHEN** offline diarization runs and the `maxSpeakers` setting is a positive value N
- **THEN** the clustering SHALL produce at most min(N, configured default ceiling, 255) distinct speaker labels

#### Scenario: Max speakers unset

- **WHEN** offline diarization runs and the `maxSpeakers` setting is unset or zero
- **THEN** the clustering SHALL apply the configured default speaker-count ceiling and infer the speaker count automatically within it

## ADDED Requirements

### Requirement: Offline diarization reports overlapping speakers
The offline pipeline SHALL assign up to two speakers to detected overlap regions, emitting temporally overlapping output segments with distinct speaker labels, and SHALL preserve single-label segments outside overlap regions.

#### Scenario: Overlap yields two speaker segments
- **WHEN** offline diarization processes a region where two speakers talk simultaneously and both are active clusters
- **THEN** the output SHALL contain overlapping-time-range segments labeled with the two distinct speakers rather than collapsing the region to one speaker

#### Scenario: Non-overlap regions stay single-labeled
- **WHEN** offline diarization processes speech from a single active speaker
- **THEN** the output segments for that region SHALL carry exactly one speaker label each

## REMOVED Requirements

### Requirement: Singleton cluster pruning
**Reason**: Superseded by resegmentation-based turn construction plus the minimum-speech filter; the vendored pipeline measures singleton pruning as net-negative for the powerset architecture (it collapses short clips into a single speaker).
**Migration**: No replacement pass is needed; short fragments are absorbed by resegmentation and sub-`min_speech_secs` regions are dropped during binarization. Over-clustering protection remains via the always-enforced speaker-count ceiling.
