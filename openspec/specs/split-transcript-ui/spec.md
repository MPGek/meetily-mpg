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
The system SHALL include `source_device` and optional `speaker`/`speaker_label` fields in the `MeetingTranscript` API response struct returned by `api_get_meeting_transcripts`.

#### Scenario: API returns source device and speaker
- **WHEN** the frontend requests meeting transcripts via `api_get_meeting_transcripts`
- **THEN** each transcript in the response SHALL include `source_device` (or NULL) and `speaker`/`speaker_label` (or NULL)

### Requirement: Frontend types carry source device
The frontend `Transcript`, `TranscriptUpdate`, and `TranscriptSegmentData` types SHALL include optional `source_device`, `speaker`, and `speaker_label` fields.

#### Scenario: TranscriptUpdate carries source device
- **WHEN** a `transcript-update` event is received from the backend
- **THEN** the `source_device` field SHALL be preserved in the frontend `Transcript` state object

#### Scenario: TranscriptSegmentData carries speaker fields
- **WHEN** transcripts are converted to `TranscriptSegmentData` for display
- **THEN** the `speaker` and `speaker_label` fields SHALL be included in the segment data when present

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
- **WHEN** the bundled enhanced diarization models are available and a meeting has saved audio
- **THEN** a "Re-analyze Speakers" button SHALL be visible in the meeting detail view

#### Scenario: Button disabled during processing
- **WHEN** diarization is running for this meeting (`diarization_status="processing"`)
- **THEN** the "Re-analyze Speakers" button SHALL show a loading state and be disabled

#### Scenario: Button hidden when models not downloaded
- **WHEN** the bundled enhanced diarization models are not available
- **THEN** the button SHALL not be visible; a "Setup Speaker Diarization" link SHALL point to settings

### Requirement: Diarization progress indicator
The transcript view SHALL display a progress indicator when diarization is running.

#### Scenario: Progress bar during diarization
- **WHEN** `diarization_status="processing"` for the current meeting
- **THEN** a progress bar with percentage and status message SHALL be displayed above the transcript list

#### Scenario: Auto-refresh after completion
- **WHEN** diarization completes and the meeting detail page is open
- **THEN** the transcript view SHALL automatically refresh to show speaker labels

### Requirement: Play button on transcript blocks in meeting details view
Transcript blocks (utterances) in the meeting details view SHALL show a play button that seeks the meeting audio player to the block's recording-relative start time and resumes playback, enabling validation of transcription text and speaker assignment.

#### Scenario: Play button shown for timed utterances
- **WHEN** a transcript block has an `audio_start_time` and the meeting has a resolvable audio file
- **THEN** the block SHALL display a play button (in all three visual variants: legacy, microphone, system)

#### Scenario: Play button hidden without audio time
- **WHEN** a transcript block has no `audio_start_time`
- **THEN** the block SHALL NOT display a play button

#### Scenario: Play button hidden when meeting has no audio
- **WHEN** the meeting has no resolvable audio file
- **THEN** no transcript block SHALL display a play button

#### Scenario: Activating play button seeks and resumes playback
- **WHEN** the user clicks the play button on a transcript block
- **THEN** the audio player SHALL seek to the block's `audio_start_time` and resume playback

### Requirement: Active block visual state during playback
The transcript block currently being played SHALL be visually highlighted with an accent style in all three visual variants (legacy, microphone, system), and its play button SHALL show a pause glyph while playing, revert to a play glyph when paused, and the highlight SHALL clear when playback ends.

#### Scenario: Playing block shows pause glyph and highlight
- **WHEN** playback is active and the position is inside a transcript block's time range
- **THEN** that block SHALL be rendered with the active highlight style and its play button SHALL show the pause glyph; all other blocks SHALL show the play glyph without highlight

#### Scenario: Pause reverts glyph, keeps highlight
- **WHEN** the player is paused
- **THEN** the highlighted block SHALL keep its highlight but its play button SHALL revert to the play glyph

#### Scenario: Playback end clears highlight
- **WHEN** playback ends naturally
- **THEN** no block SHALL show the active highlight or pause glyph

#### Scenario: Highlight applies to mic and system blocks alike
- **WHEN** the position is inside a block of any visual variant (legacy, microphone, or system)
- **THEN** that block SHALL receive the active highlight styling

### Requirement: Adaptive transcript content width
The transcript views SHALL render transcript content using the full available panel width, limited only by a maximum readable column width of 750px, and chat-style bubbles (microphone and system variants) SHALL span at least 90% of the available row width.

#### Scenario: Narrow window live view uses full panel width
- **WHEN** the live transcription view is shown in a narrow window (panel width below ~1100px)
- **THEN** the transcript content column SHALL occupy the full panel width minus only the container's base padding, with no fixed proportional (2/3) margin

#### Scenario: Wide window keeps readable line length
- **WHEN** the live transcription view panel is wider than 750px
- **THEN** the transcript content column SHALL be capped at 750px and centered, preserving comfortable line length

#### Scenario: Chat bubbles keep source-side cue with relaxed cap
- **WHEN** a microphone or system segment is rendered as a bubble in any transcript view
- **THEN** the bubble SHALL span at least 90% of the available row width and SHALL remain aligned to its source side (microphone left, system right)

#### Scenario: Meeting details panel reuses the same width rules
- **WHEN** a meeting details view renders transcript segments in its side panel
- **THEN** the bubble width rules above SHALL apply identically to the live view

### Requirement: Live auto-follow control
The live transcription view SHALL follow the bottom with new segments only while the view is pinned to the bottom, SHALL never yank the view down after the user scrolls up, and SHALL offer a visible control to return to the live bottom.

#### Scenario: Scrolled-up view stays put on new segments
- **WHEN** the user has scrolled up in the live transcript view and a new transcript segment arrives
- **THEN** the view SHALL NOT move, including when the user scrolls up within the auto-scroll delay window after the segment arrived

#### Scenario: Scroll-to-bottom button appears when not pinned
- **WHEN** the live transcript view is not pinned to the bottom
- **THEN** a circular overlay button with a downward arrow SHALL be shown at the bottom-right of the transcript panel, vertically aligned with the recording controls

#### Scenario: Button click returns to live bottom
- **WHEN** the user clicks the scroll-to-bottom button
- **THEN** the view SHALL scroll to the bottom, auto-follow SHALL be re-enabled, and the button SHALL be hidden

#### Scenario: User scroll during auto-scroll animation takes over
- **WHEN** the user scrolls up while the live view is programmatically scrolling to the bottom
- **THEN** the animation SHALL be abandoned, auto-follow SHALL pause immediately, and the button SHALL appear without delay

#### Scenario: Manual scroll to bottom restores auto-follow
- **WHEN** the user manually scrolls the live transcript view back to the bottom
- **THEN** auto-follow SHALL be re-enabled and the button SHALL be hidden

#### Scenario: Button hidden during programmatic scroll
- **WHEN** the view is scrolling to the bottom under its own control (auto-follow or button click)
- **THEN** the button SHALL NOT flicker into view during that scroll
