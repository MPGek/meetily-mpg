## MODIFIED Requirements

### Requirement: Alignment model catalog and download
The system SHALL provide a catalog of word-alignment models with metadata (name, language coverage, size) and SHALL support downloading a selected model with progress reporting, following the same model-management UX pattern as the Parakeet engine. Every catalogued file SHALL have an exact byte size and a version-pinned source. A model SHALL be available only when every file has exactly that size. Downloads SHALL resume validated partial files and SHALL be cancellable with a `cancelled`/`pending` result. The system SHALL report alignment-model readiness (available / missing) independently of transcription models.

#### Scenario: List alignment models in settings
- **WHEN** the user opens the word-alignment section of settings
- **THEN** the system displays the available alignment models with size and language metadata and a download control per model

#### Scenario: Download alignment model
- **WHEN** the user clicks "Download" on an alignment model
- **THEN** the system downloads the model files with progress reporting and marks the model available when complete

#### Scenario: Alignment requested without a downloaded model
- **WHEN** alignment is requested but no alignment model has been downloaded
- **THEN** the system SHALL report the model as unavailable and SHALL NOT fail the enclosing transcription or diarization operation

#### Scenario: Truncated model file is not available
- **WHEN** a catalogued file on disk is smaller than its exact catalogued size, even if only slightly
- **THEN** the model SHALL NOT be reported available and alignment SHALL fall back as for a missing model

#### Scenario: Resume after interruption keeps finished files
- **WHEN** an alignment download is interrupted after some files finished and the user downloads again
- **THEN** finished files SHALL NOT be requested again, and the interrupted file SHALL resume only after the server's partial response reports a range that continues the local file and ends at its exact size

#### Scenario: Cancel and immediate retry
- **WHEN** the user cancels an alignment download and immediately starts it again
- **THEN** the cancel request SHALL return `cancelled` once the download has stopped (or `pending` after 5 seconds), the partial files SHALL be kept for resume, and at most one download SHALL write the model's files at any time

#### Scenario: Pinned source revision
- **WHEN** an alignment model is downloaded
- **THEN** the request URLs SHALL reference a fixed repository revision rather than a moving branch
