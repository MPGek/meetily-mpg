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
The system SHALL download Whisper model files from Hugging Face (ggerganov/whisper.cpp) with progress reporting.

#### Scenario: Download tiny model
- **WHEN** user clicks "Download" on the tiny model
- **THEN** system streams the file from Hugging Face and reports 0–100% progress to UI

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
The system SHALL cancel an in-progress model download when requested by the user.

#### Scenario: Cancel download mid-flight
- **WHEN** user clicks "Cancel" during a model download
- **THEN** system aborts the HTTP stream, removes partial file, and resets status to Missing

### Requirement: Parallel batch processing
The system SHALL support parallel transcription of multiple audio chunks using configurable worker count.

#### Scenario: Process 10 audio chunks in parallel
- **WHEN** user initiates batch transcribe on a saved recording
- **THEN** system splits into parallel workers based on available CPU cores and processes chunks concurrently

