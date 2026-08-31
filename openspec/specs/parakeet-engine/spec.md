# parakeet-engine Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: Parakeet model catalog
The system SHALL provide a curated catalog of available Parakeet (NVIDIA NeMo) models with metadata including quantization type (Int8/FP32), speed rating, and description.

#### Scenario: List available Parakeet models
- **WHEN** user opens the transcription engine selector in settings
- **THEN** system displays all known Parakeet models with their quantization, accuracy, and speed ratings

### Requirement: Model download with resume support
The system SHALL download Parakeet model files from Hugging Face as a multi-file directory (encoder, decoder, preprocessor, vocab) with progress reporting and HTTP range-resume.

#### Scenario: Download parakeet-tdt-0.6b-v3-int8 model
- **WHEN** user clicks "Download" on the v3 Int8 model
- **THEN** system downloads 4 files (encoder-model.int8.onnx, decoder_joint-model.int8.onnx, nemo128.onnx, vocab.txt) and reports weighted progress

#### Scenario: Resume interrupted download
- **WHEN** download is interrupted at 60% and user resumes
- **THEN** system sends Range header to continue from the last byte received

### Requirement: Model loading with ONNX Runtime
The system SHALL load a Parakeet model into memory using ONNX Runtime with Int8 or FP32 quantization support.

#### Scenario: Load Int8 quantized model
- **WHEN** user selects parakeet-tdt-0.6b-v3-int8 and clicks "Load"
- **THEN** system loads the encoder and decoder joint models via ONNX Runtime in Int8 mode

### Requirement: Audio transcription via Parakeet
The system SHALL transcribe 16kHz audio samples using the loaded Parakeet model and SHALL return, alongside the transcript text, per-word timestamps derived from the model's native token-frame alignment (not linear interpolation): each emitted word SHALL carry a start and end time in seconds, relative to the start of the transcribed chunk, quantized to the model's frame granularity. Word timestamps SHALL be non-decreasing across the sequence, and the last word's end time SHALL NOT exceed the chunk duration by more than one frame.

#### Scenario: Transcribe a recording chunk
- **WHEN** recording produces an audio chunk and Parakeet model is loaded
- **THEN** system returns cleaned transcript text from the ONNX inference pipeline

#### Scenario: Chunk transcription carries word timestamps
- **WHEN** the transcription worker receives a Parakeet result for an audio chunk
- **THEN** the published transcript update SHALL include one token per emitted word with frame-aligned start/end times offset to absolute recording time, in the same form Whisper chunks publish

#### Scenario: Empty transcription
- **WHEN** Parakeet returns no text for a chunk
- **THEN** the transcript update SHALL carry no tokens (an empty or absent token list), consistent with the empty text

### Requirement: Model management (delete, validate)
The system SHALL allow deleting downloaded models and validating directory integrity by checking all required files exist with minimum sizes.

#### Scenario: Delete a corrupted model directory
- **WHEN** user clicks "Delete" on a corrupted Parakeet model in settings
- **THEN** system removes the entire model directory from disk and updates status to Missing

### Requirement: Model cancellation support
The system SHALL cancel an in-progress model download when requested by the user, cleaning up partial files.

#### Scenario: Cancel download mid-flight
- **WHEN** user clicks "Cancel" during a Parakeet model download
- **THEN** system aborts HTTP streams, removes partial directory contents, and resets status to Missing

### Requirement: Download timeout handling
The system SHALL detect stalled downloads (no data for 30 seconds) and report connection errors with actionable messages.

#### Scenario: Detect stalled connection
- **WHEN** no bytes received for 30 seconds during model download
- **THEN** system aborts the download and returns a "Connection timeout" error to UI

