# Delta spec: online-speaker-diarization

## MODIFIED Requirements

### Requirement: Graceful fallback to offline diarization
The system SHALL fall back to the existing offline diarization pipeline if online processing fails for any reason. Online initialization SHALL use the same 3-location enhanced model resolver as offline diarization (`app_data_dir/models` → `resource_dir/models` → `CARGO_MANIFEST_DIR/models`), and failures SHALL report all searched locations.

#### Scenario: Streaming diarization initialization failure
- **WHEN** the polyvoice `StreamingPipeline` fails to initialize in Fast mode because the bundled enhanced model files are absent or corrupt in all fallback locations
- **THEN** the system SHALL log the error with all searched directories, emit a warning event to the frontend noting that the enhanced models are bundled at build time near the executable, and continue recording without diarization; offline diarization remains available after recording stops (but also fails if the enhanced models are missing in all locations)

#### Scenario: Embedding extraction runtime error
- **WHEN** the enhanced embedding model fails during recording in Efficient mode
- **THEN** the system SHALL log the error, clear the embedding buffer, and allow offline diarization to run after recording stops

## ADDED Requirements

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
