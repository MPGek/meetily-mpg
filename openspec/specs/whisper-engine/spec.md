# whisper-engine Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: Whisper model catalog
The system SHALL provide a curated catalog of available Whisper models with metadata (name, size, accuracy tier, speed tier, description).

#### Scenario: List available Whisper models
- **WHEN** user opens the model selector in settings
- **THEN** system displays all known Whisper models with their accuracy and speed ratings

### Requirement: Model download
The system SHALL download Whisper model files from Hugging Face (ggerganov/whisper.cpp) with progress reporting, identifying itself with a versioned `Meetily/<version>` User-Agent. At most one download per model SHALL be active at a time. A downloaded file SHALL become Available only after it passes the GGML/GGUF header check and is at least 90% of its catalogued size. On any download failure the partial file SHALL be removed and the model SHALL return to Missing.

#### Scenario: Download tiny model
- **WHEN** user clicks "Download" on the tiny model
- **THEN** system streams the file from Hugging Face and reports 0–100% progress to UI

#### Scenario: Truncated or invalid file is not published
- **WHEN** the response ends early or the downloaded file lacks a valid GGML/GGUF header
- **THEN** the download SHALL fail with an error, the partial file SHALL be removed, and the model SHALL be reported Missing, not Available

#### Scenario: Network error releases the download
- **WHEN** the request or the response stream fails partway through
- **THEN** a new download of the same model SHALL be accepted immediately afterwards and SHALL NOT be rejected as already in progress

#### Scenario: Status shown while a download is in progress
- **WHEN** the model list is requested while a Whisper download is active
- **THEN** that model SHALL be reported as Downloading with its last known percent, serialized as `{ "Downloading": { "progress": <n> } }`, and the settings UI SHALL display that percent

### Requirement: Model loading with hardware-adaptive config
The system SHALL load a Whisper model into memory using hardware-detected GPU acceleration parameters.

#### Scenario: Load large-v3-turbo on Metal-equipped Mac
- **WHEN** user selects large-v3-turbo and app detects Apple Silicon
- **THEN** system enables Metal GPU acceleration with optimized beam size and thread count

### Requirement: Audio transcription via Whisper
The system SHALL transcribe 16kHz audio chunks using the loaded Whisper model.

#### Scenario: Transcribe a 2-second audio chunk
- **WHEN** recording produces an audio chunk and Whisper model is loaded
- **THEN** system returns cleaned transcript text with confidence score

### Requirement: Language support (auto, auto-translate, explicit)
The system SHALL support three language modes: automatic detection, auto-detect + translate to English, and explicit language code. The currently selected mode SHALL be visible on the home-page `Language` button as a short code (`(xx)`, `(auto)`, `(auto-en)` for auto-translate).

#### Scenario: Transcribe in French with auto mode
- **WHEN** user sets language preference to "auto" for a recording session
- **THEN** Whisper detects French automatically and transcribes in French

#### Scenario: Transcribe Japanese and translate to English
- **WHEN** user sets language preference to "auto-translate"
- **THEN** Whisper transcribes in Japanese and translates output to English

#### Scenario: Selected mode visible on home without opening settings
- **WHEN** user is on the home page with any language mode selected
- **THEN** the `Language` button displays the short code of that mode (`(auto)`, `(auto-en)`, or explicit code such as `(ru)`)

### Requirement: Model management (delete, validate)
The system SHALL allow deleting downloaded models and validating GGML file integrity.

#### Scenario: Delete a corrupted model
- **WHEN** user clicks "Delete" on a corrupted model in settings
- **THEN** system removes the model file from disk and updates status to Missing

### Requirement: Model cancellation support
The system SHALL cancel an in-progress model download when requested by the user. The cancel request SHALL return `cancelled` once the download has stopped, removed its partial file and released its claim on the model, or `pending` if that has not happened within 5 seconds. While a cancellation is pending, the system SHALL reject a new download of the same model.

#### Scenario: Cancel download mid-flight
- **WHEN** user clicks "Cancel" during a model download
- **THEN** system aborts the HTTP stream, removes partial file, resets status to Missing, emits a progress event with status `cancelled`, and the cancel request returns `cancelled`

#### Scenario: Cancel and immediate retry
- **WHEN** the user cancels a download and immediately starts it again
- **THEN** the new download SHALL start only after the cancelled one has removed its partial file, and at most one download SHALL write the model file at any time

#### Scenario: Cleanup slower than the acknowledgement bound
- **WHEN** a cancelled download has not finished stopping within 5 seconds
- **THEN** the cancel request SHALL return `pending`, the model SHALL keep reporting Downloading, and the UI SHALL keep retry disabled until the `cancelled` event arrives

#### Scenario: Download completes before cancellation takes effect
- **WHEN** a cancel request arrives after the download has already completed and released its claim
- **THEN** the model SHALL stay Available and its file SHALL NOT be removed

### Requirement: Parallel batch processing
The system SHALL support parallel transcription of multiple audio chunks using configurable worker count.

#### Scenario: Process 10 audio chunks in parallel
- **WHEN** user initiates batch transcribe on a saved recording
- **THEN** system splits into parallel workers based on available CPU cores and processes chunks concurrently
