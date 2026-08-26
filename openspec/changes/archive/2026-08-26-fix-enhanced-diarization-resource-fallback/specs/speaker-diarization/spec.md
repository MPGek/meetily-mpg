# Delta spec: speaker-diarization

## MODIFIED Requirements

### Requirement: Diarization model management
The system SHALL support the enhanced diarization model set (pyannote `segmentation-3.0` + TitaNet-Large) bundled at build time, resolving model files through a 3-location fallback chain — `app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models` (dev) — and verified by file existence and size (>1 KB; SHA-256 when published). There SHALL be no runtime download or removal capability. The `check_diarization_models` command and the diarization engine SHALL use the same resolver so their readiness decisions never disagree. The system SHALL remove stale model files from the model directory when model status is checked.

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

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using the enhanced polyvoice ONNX model set (pyannote `segmentation-3.0` segmentation, TitaNet-Large speaker embedding, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL resolve its segmentation and embedding models through the 3-location fallback chain and SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio and the enhanced models are present in any fallback location
- **THEN** the system runs segmentation, embedding extraction, and clustering via the resolved model directory, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

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
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

## ADDED Requirements

### Requirement: Enhanced model resolution across install locations
The system SHALL resolve the enhanced diarization model directory by searching `app_data_dir/models`, then `resource_dir/models` (Tauri bundled resources next to the executable), then `CARGO_MANIFEST_DIR/models` (dev), in that priority order. The first location where *both* `segmentation-3.0.onnx` and `titanet_large.onnx` pass `verify_enhanced_integrity` (existence + size >1 KB) SHALL be selected as the active models directory. `app_data` takes priority so a user-placed override wins; `resource_dir` is the production bundle location; `manifest` is dev fallback.

#### Scenario: Resolve from bundled resources when AppData empty
- **WHEN** offline diarization is triggered and `app_data_dir/models` does not contain the enhanced files but `resource_dir/models` does
- **THEN** the system SHALL load segmentation and embedding directly from `resource_dir/models` and diarization SHALL succeed without copying files

#### Scenario: Resolve from AppData when both locations present
- **WHEN** both `app_data_dir/models` and `resource_dir/models` contain verified enhanced files
- **THEN** the system SHALL load from `app_data_dir/models`

#### Scenario: All locations missing lists searched paths
- **WHEN** offline diarization is triggered and no fallback location contains verified enhanced files
- **THEN** the error SHALL name all three searched directories and explain that the models are bundled at build time near the executable and that a rebuild with network or an installer that includes them is required

#### Scenario: Settings and engine agree
- **WHEN** `check_diarization_models` reports `ready=true`
- **THEN** a subsequent diarization run SHALL succeed (and vice versa: `ready=false` implies diarization will fail), because both use the same resolver and verification logic

#### Scenario: Dev manifest fallback
- **WHEN** the app runs in development (`cargo tauri dev`) and the enhanced files are present only in `frontend/src-tauri/models/`
- **THEN** diarization SHALL succeed via the manifest fallback location without requiring files in AppData or resources
