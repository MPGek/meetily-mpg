# database Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: SQLite data layer with SQLx
The system SHALL use SQLx for async SQLite operations with connection pooling and type-safe query construction.

#### Scenario: Query meetings from database
- **WHEN** user opens the meetings list in the app
- **THEN** system queries all meeting records from the `meetings` table ordered by created_at descending

### Requirement: Meeting CRUD operations
The system SHALL support full create/read/update/delete of meeting records with UUID-based IDs.

#### Scenario: Create a new meeting record
- **WHEN** recording starts, system creates a meeting record with generated UUID and folder path
- **THEN** the meeting is stored in the `meetings` table with created_at timestamp

### Requirement: Transcript storage and retrieval
The system SHALL store transcript segments associated with meetings, including audio timing metadata.

#### Scenario: Save a transcript segment during recording
- **WHEN** Whisper/Parakeet produces a text chunk during active recording
- **THEN** system stores the segment in `transcripts` table linked to the meeting_id with start/end timestamps

### Requirement: Summary process tracking
The system SHALL track summary generation jobs per meeting with status, result JSON, and timing metadata.

#### Scenario: Record summary completion
- **WHEN** a summary finishes generating via LLM provider
- **THEN** system updates the `summary_processes` table with completed status, result JSON, chunk count, and processing duration

### Requirement: Settings storage for API keys and configs
The system SHALL persist provider configurations (API keys, endpoints, model selections) in the settings table.

#### Scenario: Save OpenAI API key
- **WHEN** user enters an OpenAI API key in settings
- **THEN** system stores it in the `settings` table under provider="openai" with encrypted or plaintext storage

### Requirement: Database initialization and migration
The system SHALL create the SQLite database schema on first launch using SQLx migrations.

#### Scenario: First-run database creation
- **WHEN** app starts and no database file exists at the configured path
- **THEN** system runs all pending migrations to create tables (meetings, transcripts, summary_processes, settings)

### Requirement: Database directory management
The system SHALL manage the database file location within the app's data directory with folder browsing support.

#### Scenario: Open database folder from settings
- **WHEN** user clicks "Open Database Folder" in settings
- **THEN** system opens the OS file explorer at the directory containing the SQLite database file

### Requirement: Speaker columns on transcripts
The `transcripts` table SHALL include `speaker TEXT` and `speaker_label TEXT` columns for storing speaker diarization results.

#### Scenario: Store speaker ID on transcript
- **WHEN** diarization assigns a speaker to a transcript segment
- **THEN** the `speaker` column SHALL contain the speaker ID (e.g., "SPEAKER_00")

#### Scenario: Store user-assigned speaker label
- **WHEN** user renames a speaker in the UI
- **THEN** the `speaker_label` column SHALL be updated with the human-readable name (e.g., "Alice")

#### Scenario: Legacy transcripts have null speaker
- **WHEN** existing transcripts without diarization are queried
- **THEN** `speaker` and `speaker_label` SHALL be NULL

### Requirement: Diarization metadata on meetings
The `meetings` table SHALL include `diarization_status TEXT` and `speaker_names TEXT` columns for tracking diarization state and speaker name mappings.

#### Scenario: Track diarization status
- **WHEN** diarization is running, succeeded, or failed for a meeting
- **THEN** `diarization_status` SHALL be set to "processing", "complete", or "failed" respectively

#### Scenario: Persist speaker name mappings
- **WHEN** user assigns names to speakers in a meeting
- **THEN** the `speaker_names` column SHALL store a JSON map of speaker IDs to labels (e.g., `{"SPEAKER_00": "Alice", "SPEAKER_01": "Bob"}`)

