## MODIFIED Requirements

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
