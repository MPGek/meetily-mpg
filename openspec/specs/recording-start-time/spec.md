# recording-start-time Specification

## Purpose

Persisting when each recording actually started so lists and views show the meeting's start time instead of the stop time the database row was created with.

## Requirements

### Requirement: Recorded meetings persist their start time

When a recorded meeting is saved, the system SHALL persist the recording's start moment (as written to the meeting folder's `metadata.json` when recording began) as the meeting's `started_at`. If the folder metadata is missing or its timestamp is unparsable, the system SHALL fall back to the save moment so `started_at` is never empty for newly saved meetings.

#### Scenario: Normal stop persists start time

- **WHEN** a recording started at `14:00` is stopped and saved at `15:30`
- **THEN** the meeting's `started_at` is `14:00` (not `15:30`).

#### Scenario: Corrupt or missing folder metadata falls back

- **WHEN** a meeting is saved whose folder has no readable `metadata.json` start timestamp
- **THEN** the meeting is still saved with `started_at` set to the save moment instead of failing or storing NULL.

#### Scenario: Crash-recovered recordings keep the original start

- **WHEN** a recording interrupted by an app crash is recovered and saved from its folder
- **THEN** the meeting's `started_at` reflects the original recording start, not the recovery moment.

### Requirement: Imported audio uses the file's modification time as start

When a meeting is created by importing an audio file, the system SHALL set `started_at` to the audio file's last-modification time, falling back to the import moment when the file time is unavailable.

#### Scenario: Import uses file mtime

- **WHEN** a user imports an audio file last modified `2026-09-10 18:05`
- **THEN** the meeting's `started_at` is `2026-09-10 18:05`.

#### Scenario: Unavailable file time falls back

- **WHEN** the audio file's modification time cannot be read
- **THEN** the meeting is still imported with `started_at` set to the import moment.

### Requirement: Meetings list exposes start time with fallback

The meetings list read path SHALL return each meeting's `started_at` alongside the existing fields, and displayed dates SHALL prefer `started_at`, falling back to `created_at` when `started_at` is absent (legacy rows, old clients).

#### Scenario: List shows start time

- **WHEN** the frontend requests the meetings list for a meeting started at `14:00` and saved at `15:30`
- **THEN** the entry carries `started_at` = `14:00` and the UI renders `14:00`.

#### Scenario: Legacy rows still render

- **WHEN** a meeting created before this feature (no `started_at`) is listed
- **THEN** the entry renders using `created_at` instead of failing or showing blank.
