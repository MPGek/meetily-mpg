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

### Requirement: Meeting tag tables with cascade cleanup
The system SHALL persist meeting tags in `meeting_tags` (`id`, `name`, `color`, timestamps) and links in `meeting_tag_links` (`meeting_id`, `tag_id`), with a case-insensitive unique index on tag name, a composite primary key on the link, foreign keys to `meetings` and `meeting_tags`, and deletion of a meeting SHALL delete its link rows while leaving dictionary entries intact.

#### Scenario: Migration creates tables
- **WHEN** the app launches on a database without tag tables
- **THEN** migration creates `meeting_tags` and `meeting_tag_links` with the unique name index and link primary key, and existing meetings remain readable with zero tags.

#### Scenario: Meeting delete cascades links only
- **WHEN** a meeting with two tag links is deleted via the meeting deletion path
- **THEN** its rows in `meeting_tag_links` are removed, the `meeting_tags` rows survive, and no orphan link rows remain.

#### Scenario: Duplicate link impossible
- **WHEN** the same (`meeting_id`, `tag_id`) pair is inserted twice
- **THEN** the database rejects the second insert via the composite primary key.

### Requirement: Meetings list query returns timestamps and tags
The meetings list read path SHALL return each meeting's `created_at` timestamp and its assigned tags (id, name, color) alongside `id` and `title`, ordered by `created_at` descending.

#### Scenario: List includes new fields
- **WHEN** the frontend requests the meetings list
- **THEN** each entry includes `created_at` and a `tags` array (possibly empty) without dropping `id` or `title`.

#### Scenario: Legacy rows without tags
- **WHEN** a meeting created before the tags feature is listed
- **THEN** it returns an empty `tags` array and a valid `created_at` instead of an error.

### Requirement: Meetings table stores recording start time

The `meetings` table SHALL include a nullable `started_at TEXT` column holding the recording's start moment (UTC RFC3339). Existing rows SHALL be backfilled with `started_at = created_at` at migration time (documented approximation: for pre-feature rows this is the stop time). The `created_at`/`updated_at` semantics and the `created_at DESC` list ordering SHALL remain unchanged.

#### Scenario: Migration backfills legacy rows

- **WHEN** the app launches on a database without `started_at`
- **THEN** migration adds the column, sets `started_at = created_at` for all existing rows, and no meeting loses its list position.

#### Scenario: New inserts always set start time

- **WHEN** a meeting is created via the transcript-save path or the audio-import path
- **THEN** its row stores a non-empty `started_at` (from folder metadata / file mtime, or the documented fallbacks).

