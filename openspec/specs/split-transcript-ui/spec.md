# split-transcript-ui Specification

## Purpose
Chat-style UI rendering for transcript segments with source-based visual differentiation.

## Requirements
### Requirement: Source device persisted in database
The system SHALL store the `source_device` field ("Microphone", "System", or NULL) for each transcript segment in the SQLite `transcripts` table.

#### Scenario: Save transcript with source device
- **WHEN** a meeting is saved with transcript segments that have `source_device` values
- **THEN** each segment's `source_device` SHALL be persisted in the `source_device` column of the `transcripts` table

#### Scenario: Load transcript with source device
- **WHEN** transcripts are loaded from the database for a meeting
- **THEN** each returned segment SHALL include its `source_device` value (or NULL for legacy segments)

### Requirement: Source device in API response
The system SHALL include `source_device` in the `MeetingTranscript` API response struct returned by `api_get_meeting_transcripts`.

#### Scenario: API returns source device
- **WHEN** the frontend requests meeting transcripts via `api_get_meeting_transcripts`
- **THEN** each transcript in the response SHALL include `source_device` (or be omitted if NULL)

### Requirement: Frontend types carry source device
The frontend `Transcript`, `TranscriptUpdate`, and `TranscriptSegmentData` types SHALL include an optional `source_device` field.

#### Scenario: TranscriptUpdate carries source device
- **WHEN** a `transcript-update` event is received from the backend
- **THEN** the `source_device` field SHALL be preserved in the frontend `Transcript` state object

#### Scenario: TranscriptSegmentData carries source device
- **WHEN** transcripts are converted to `TranscriptSegmentData` for display
- **THEN** the `source_device` field SHALL be included in the segment data

### Requirement: Chat-style layout for live transcription
The live transcription view (home page) SHALL render transcript segments in a chat-style layout with visual differentiation by source.

#### Scenario: Microphone segment rendered left-aligned
- **WHEN** a transcript segment has `source_device` equal to "Microphone"
- **THEN** the segment SHALL be rendered left-aligned with the timestamp on the left side and a blue-tinted background bubble

#### Scenario: System segment rendered right-aligned
- **WHEN** a transcript segment has `source_device` equal to "System"
- **THEN** the segment SHALL be rendered right-aligned with the timestamp on the right side and a green-tinted background bubble

#### Scenario: Legacy segment rendered neutrally
- **WHEN** a transcript segment has no `source_device` value (NULL or undefined)
- **THEN** the segment SHALL be rendered with the existing neutral style (left-aligned, no background bubble)

### Requirement: Chat-style layout for meeting details
The meeting details view (historical meetings) SHALL use the same chat-style layout as the live transcription view.

#### Scenario: Historical meeting displays chat layout
- **WHEN** a user views a past meeting's transcripts
- **THEN** segments SHALL be rendered with the same source-based alignment and background colors as the live view

#### Scenario: Historical meeting with no source data
- **WHEN** a user views a past meeting that was recorded before `source_device` tracking
- **THEN** all segments SHALL render with the neutral legacy style

### Requirement: Source device preserved through state management
The `TranscriptContext` SHALL preserve `source_device` through all state operations including buffering, sorting, deduplication, and reload sync.

#### Scenario: Source device survives buffer processing
- **WHEN** transcript updates are buffered and processed into state
- **THEN** the `source_device` field SHALL be preserved on each transcript

#### Scenario: Source device survives reload sync
- **WHEN** transcripts are synced from backend after a page reload during active recording
- **THEN** the `source_device` field SHALL be included in the synced transcript objects
