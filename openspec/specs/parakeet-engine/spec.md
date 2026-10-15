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
The system SHALL download Parakeet model files as a multi-file directory (encoder, decoder, preprocessor, vocab) from a version-pinned source, with progress reporting and HTTP range-resume. Every catalogued file SHALL have an exact byte size. A local file SHALL count as complete only at exactly that size. A resumed response SHALL be accepted only if its reported byte range continues the local file and ends at the exact size. A model SHALL become Available only when every file has exactly its catalogued size.

#### Scenario: Download parakeet-tdt-0.6b-v3-int8 model
- **WHEN** user clicks "Download" on the v3 Int8 model
- **THEN** system downloads 4 files (encoder-model.int8.onnx, decoder_joint-model.int8.onnx, nemo128.onnx, vocab.txt) and reports weighted progress

#### Scenario: Resume interrupted download
- **WHEN** download is interrupted at 60% and user resumes
- **THEN** system sends Range header to continue from the last byte received, and appends only after the server's partial response reports a range starting at that byte and ending at the file's exact size

#### Scenario: Completed file survives a failure in a later file
- **WHEN** the encoder has finished at its exact size, the next file fails (network loss or an HTTP error), and the user retries
- **THEN** the encoder SHALL NOT be deleted or requested again, and only the unfinished files SHALL be fetched

#### Scenario: Server ignores the range request
- **WHEN** a resume request receives a full `200` response instead of a partial one
- **THEN** the system SHALL replace the partial file from byte zero and SHALL report progress from the bytes actually confirmed, not from the discarded prefix

#### Scenario: Server rejects the range as unsatisfiable
- **WHEN** a resume request receives `416` reporting the file's exact total size
- **THEN** the system SHALL fetch the file again from byte zero, and SHALL fail the download if the `416` reports a different total

#### Scenario: Invalid or overlong response never publishes Available
- **WHEN** a response declares a length or byte range that does not match the catalogued size, or delivers more bytes than the catalogued size
- **THEN** the download SHALL fail with an error, the model SHALL NOT be reported Available, and bytes already confirmed SHALL be kept for a later resume

#### Scenario: Pinned source revision
- **WHEN** a Hugging Face-hosted Parakeet model is downloaded
- **THEN** the request URLs SHALL reference a fixed repository revision rather than a moving branch

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
The system SHALL allow deleting downloaded models and validating directory integrity by checking that every required file exists with exactly its catalogued byte size.

#### Scenario: Delete a corrupted model directory
- **WHEN** user clicks "Delete" on a corrupted Parakeet model in settings
- **THEN** system removes the entire model directory from disk and updates status to Missing

#### Scenario: File one byte short of its catalogued size
- **WHEN** model discovery finds a Parakeet directory in which one file is smaller or larger than its catalogued size
- **THEN** the model SHALL be reported Corrupted (not Available), and starting a download for it SHALL keep the files that have their exact size, resume the shorter file, and re-fetch a file that is larger than its catalogued size

### Requirement: Model cancellation support
The system SHALL cancel an in-progress model download when requested by the user. Partial files SHALL be preserved so a later download resumes them. The cancel request SHALL return `cancelled` once the download has stopped and released its claim on the model, or `pending` if that has not happened within 5 seconds. While a cancellation is pending, the system SHALL reject a new download of the same model.

#### Scenario: Cancel download mid-flight
- **WHEN** user clicks "Cancel" during a Parakeet model download
- **THEN** system aborts HTTP streams, keeps the bytes already written, resets status to Missing, emits a progress event with status `cancelled`, and the cancel request returns `cancelled`

#### Scenario: Cancel near completion then retry
- **WHEN** a download is cancelled while its last file is partly written and the user then retries
- **THEN** the retry SHALL resume that file from its last written byte and SHALL NOT re-fetch files that were already complete

#### Scenario: Cleanup slower than the acknowledgement bound
- **WHEN** a cancelled download has not finished stopping within 5 seconds
- **THEN** the cancel request SHALL return `pending`, the model SHALL keep reporting Downloading, and a download or retry request for that model SHALL be rejected until the cancelled download has stopped

#### Scenario: Cancel and immediate retry
- **WHEN** the user cancels and immediately retries the same model
- **THEN** at most one download SHALL write the model's files at any time

#### Scenario: Download completes before cancellation takes effect
- **WHEN** a cancel request arrives after the download has already released its claim as complete
- **THEN** the model SHALL stay Available and the cancel request SHALL return `cancelled`

### Requirement: Download timeout handling
The system SHALL detect stalled downloads (no data for 30 seconds), abort the transfer, and report connection errors with actionable messages. Bytes received before the stall SHALL be preserved for resume, and the model SHALL return to Missing so a retry can start.

#### Scenario: Detect stalled connection
- **WHEN** no bytes received for 30 seconds during model download
- **THEN** system aborts the download, keeps the bytes already written, returns a timeout error to UI, and a subsequent retry resumes from the preserved bytes

### Requirement: Model catalog stays usable during model loading
The system SHALL keep the Parakeet model catalog readable and writable while a model is being loaded into ONNX Runtime, and SHALL serialize model load and unload so that an unload requested during a load takes effect only after that load has finished.

#### Scenario: Catalog query during a slow native load
- **WHEN** a Parakeet model load is in progress and the UI requests the model list or a download updates a model's status
- **THEN** the request SHALL complete without waiting for the load to finish

#### Scenario: Unload requested while a load is in progress
- **WHEN** an unload is requested while a Parakeet model load has not yet finished
- **THEN** the unload SHALL wait for the load to finish and SHALL then leave no model loaded
