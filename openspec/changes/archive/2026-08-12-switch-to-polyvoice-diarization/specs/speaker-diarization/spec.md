## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using polyvoice ONNX models (powerset segmentation, speaker embedding extraction, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio
- **THEN** the system runs segmentation, embedding extraction, and clustering, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use polyvoice components (streaming pipeline with a polyvoice ONNX embedder and AHC clustering) with no sherpa-onnx code or models in any diarization path

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
