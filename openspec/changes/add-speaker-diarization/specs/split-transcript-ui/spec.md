# split-transcript-ui Specification — Delta

## ADDED Requirements

### Requirement: Speaker-based visual differentiation
The transcript view SHALL visually differentiate speaker segments using color-coded speaker bands and optional grouping.

#### Scenario: Speaker segments rendered with color bands
- **WHEN** transcript segments have `speaker` values
- **THEN** each segment SHALL be rendered with a colored left-border and a speaker dot/name header, using a distinct color for each speaker

#### Scenario: Color palette cycles for many speakers
- **WHEN** more than 8 distinct speakers are present in a meeting
- **THEN** the system SHALL cycle through the color palette, ensuring each speaker gets a visually distinct color

#### Scenario: Legacy segments without speaker
- **WHEN** transcript segments have no `speaker` value (NULL)
- **THEN** segments SHALL render with the existing source_device-based styling without speaker coloration

### Requirement: Speaker label display and editing
The transcript view SHALL display speaker labels and allow inline editing.

#### Scenario: Display speaker ID as default label
- **WHEN** a segment has `speaker="SPEAKER_01"` and no `speaker_label`
- **THEN** the UI SHALL display "Speaker 01" (formatted from the ID)

#### Scenario: Display custom speaker label
- **WHEN** a segment has `speaker_label="Alice"`
- **THEN** the UI SHALL display "Alice" instead of the raw speaker ID

#### Scenario: Inline speaker rename
- **WHEN** user clicks on a speaker label and types a new name
- **THEN** the system SHALL update `speaker_label` on all transcripts with that speaker ID and persist the change via a Tauri command

### Requirement: Speaker grouping sections
The transcript view SHALL support grouping transcript segments by speaker with collapsible sections.

#### Scenario: Segments grouped by speaker
- **WHEN** speaker grouping is enabled and segments have speaker values
- **THEN** consecutive segments from the same speaker SHALL be visually grouped under a single speaker header

#### Scenario: Collapse speaker group
- **WHEN** user clicks a speaker group header
- **THEN** the segments in that group SHALL collapse, showing only the header

### Requirement: Re-analyze Speakers button
The meeting details page SHALL include a "Re-analyze Speakers" button that triggers diarization on the current meeting.

#### Scenario: Button visible when models are available
- **WHEN** diarization models are downloaded and a meeting has saved audio
- **THEN** a "Re-analyze Speakers" button SHALL be visible in the meeting detail view

#### Scenario: Button disabled during processing
- **WHEN** diarization is running for this meeting (`diarization_status="processing"`)
- **THEN** the "Re-analyze Speakers" button SHALL show a loading state and be disabled

#### Scenario: Button hidden when models not downloaded
- **WHEN** diarization models have not been downloaded
- **THEN** the button SHALL not be visible; a "Setup Speaker Diarization" link SHALL point to settings

### Requirement: Diarization progress indicator
The transcript view SHALL display a progress indicator when diarization is running.

#### Scenario: Progress bar during diarization
- **WHEN** `diarization_status="processing"` for the current meeting
- **THEN** a progress bar with percentage and status message SHALL be displayed above the transcript list

#### Scenario: Auto-refresh after completion
- **WHEN** diarization completes and the meeting detail page is open
- **THEN** the transcript view SHALL automatically refresh to show speaker labels

## MODIFIED Requirements

### Requirement: Chat-style layout for live transcription
The live transcription view (home page) SHALL render transcript segments in a chat-style layout with visual differentiation by source and optional speaker identification.

#### Scenario: Microphone segment with speaker rendered left-aligned
- **WHEN** a transcript segment has `source_device` equal to "Microphone" and has a `speaker` value
- **THEN** the segment SHALL be rendered left-aligned with the timestamp on the left side, a blue-tinted background bubble, and a speaker label/dot above the text

#### Scenario: Microphone segment without speaker rendered as before
- **WHEN** a transcript segment has `source_device` equal to "Microphone" but no `speaker` value
- **THEN** the segment SHALL be rendered left-aligned with the timestamp on the left side and a blue-tinted background bubble, without speaker labeling

#### Scenario: System segment rendered right-aligned
- **WHEN** a transcript segment has `source_device` equal to "System"
- **THEN** the segment SHALL be rendered right-aligned with the timestamp on the right side and a green-tinted background bubble

#### Scenario: Legacy segment rendered neutrally
- **WHEN** a transcript segment has no `source_device` value (NULL or undefined)
- **THEN** the segment SHALL be rendered with the existing neutral style (left-aligned, no background bubble)

### Requirement: Source device in API response
The system SHALL include `source_device` and optional `speaker`/`speaker_label` fields in the `MeetingTranscript` API response struct returned by `api_get_meeting_transcripts`.

#### Scenario: API returns source device and speaker
- **WHEN** the frontend requests meeting transcripts via `api_get_meeting_transcripts`
- **THEN** each transcript in the response SHALL include `source_device` (or NULL) and `speaker`/`speaker_label` (or NULL)

### Requirement: Frontend types carry source device
The frontend `Transcript`, `TranscriptUpdate`, and `TranscriptSegmentData` types SHALL include optional `source_device`, `speaker`, and `speaker_label` fields.

#### Scenario: TranscriptSegmentData carries speaker fields
- **WHEN** transcripts are converted to `TranscriptSegmentData` for display
- **THEN** the `speaker` and `speaker_label` fields SHALL be included in the segment data when present
