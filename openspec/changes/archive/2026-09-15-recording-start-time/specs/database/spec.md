## ADDED Requirements

### Requirement: Meetings table stores recording start time

The `meetings` table SHALL include a nullable `started_at TEXT` column holding the recording's start moment (UTC RFC3339). Existing rows SHALL be backfilled with `started_at = created_at` at migration time (documented approximation: for pre-feature rows this is the stop time). The `created_at`/`updated_at` semantics and the `created_at DESC` list ordering SHALL remain unchanged.

#### Scenario: Migration backfills legacy rows

- **WHEN** the app launches on a database without `started_at`
- **THEN** migration adds the column, sets `started_at = created_at` for all existing rows, and no meeting loses its list position.

#### Scenario: New inserts always set start time

- **WHEN** a meeting is created via the transcript-save path or the audio-import path
- **THEN** its row stores a non-empty `started_at` (from folder metadata / file mtime, or the documented fallbacks).
