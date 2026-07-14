## ADDED Requirements

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
