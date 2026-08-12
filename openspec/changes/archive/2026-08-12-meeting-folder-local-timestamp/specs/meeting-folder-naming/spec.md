## ADDED Requirements

### Requirement: Meeting folder name contains a single local-time timestamp

The system SHALL name meeting recording folders with exactly one timestamp, formatted `YYYY-MM-DD_HH-MM` in the local time zone of the machine.

#### Scenario: New recording folder with default meeting title
- **WHEN** user starts a recording on 2026-08-12 at 18:44 local time
- **THEN** the recording folder is created as `Meeting 2026-08-12_18-44` (local time, single timestamp)

#### Scenario: Folder timestamp uses local time, not UTC
- **WHEN** user starts a recording at 18:44 local time (UTC+3)
- **THEN** the folder timestamp reads `18-44`, not the UTC value `15-44`

### Requirement: Timestamp suffix appended only when the meeting name lacks one

The system SHALL append the `_YYYY-MM-DD_HH-MM` local-time suffix to the meeting folder name only when the meeting name does not already end with a `YYYY-MM-DD_HH-MM` timestamp.

#### Scenario: Default title already ends with a timestamp
- **WHEN** the meeting name is `Meeting 2026-08-12_18-44`
- **THEN** the folder is created as `Meeting 2026-08-12_18-44` without an additional suffix

#### Scenario: User-renamed title without a timestamp
- **WHEN** the meeting name is `Team sync`
- **THEN** the folder is created as `Team sync_2026-08-12_18-44` with the local-time suffix appended

### Requirement: Unique folder for recordings started in the same minute

The system SHALL guarantee a unique folder name when the resolved name already exists, by appending a numeric counter (`_1`, `_2`, ...) until a free name is found.

#### Scenario: Two recordings started in the same minute
- **WHEN** a second recording starts in the same minute as a first recording named `Meeting 2026-08-12_18-44`
- **THEN** the second folder is created as `Meeting 2026-08-12_18-44_1`

### Requirement: Auto-generated meeting titles use the local timestamp format

The system SHALL generate default meeting titles in the local `YYYY-MM-DD_HH-MM` format (`Meeting 2026-08-12_18-44`) for all recording start paths, including UI starts and tray/global-shortcut starts.

#### Scenario: Recording started from the app UI
- **WHEN** user clicks "Start Recording"
- **THEN** the meeting title is generated as `Meeting YYYY-MM-DD_HH-MM` in local time

#### Scenario: Recording started from tray without a name
- **WHEN** recording starts via the tray or global shortcut without an explicit meeting name
- **THEN** the backend fallback title is generated as `Meeting YYYY-MM-DD_HH-MM` in local time
