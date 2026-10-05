# Spec Delta

## Purpose

Defines where and how the home-screen recording controls and pending-tag picker are laid out, so that they stay reachable without ever hiding the live transcript the user is reading.

## ADDED Requirements

### Requirement: Recording footer docked below the transcript
The home screen SHALL render the recording controls and the pending-tag picker in a footer that sits below the transcript in the page layout, not floating over it. The transcript's visible area SHALL end at the footer's top edge, so no transcript content can be drawn underneath the footer, whatever the footer's current height.

#### Scenario: Newest block fully visible while recording
- **WHEN** a recording is in progress and the live transcript view is pinned to the bottom
- **THEN** the newest transcript block SHALL be fully visible above the footer, with no part of it covered by the controls or the tag picker

#### Scenario: Taller footer shrinks the transcript instead of covering it
- **WHEN** the footer grows taller (for example, while a device error is shown)
- **THEN** the transcript's visible area SHALL shrink by the same amount and the newest block SHALL remain uncovered

#### Scenario: Footer before recording starts
- **WHEN** the home screen is idle, microphone permission is granted, and no recording is in progress
- **THEN** the footer SHALL show the start-recording control and the pending-tag picker in the same docked position

#### Scenario: No footer without microphone access
- **WHEN** microphone permission is not granted and no recording is in progress
- **THEN** the footer SHALL NOT be shown and the transcript area SHALL take the full height

#### Scenario: Footer follows the sidebar width
- **WHEN** the sidebar is collapsed or expanded
- **THEN** the footer SHALL span the main content area beside the sidebar and SHALL NOT be overlapped by it

### Requirement: Controls and tag picker share one row
The footer SHALL place the recording controls and the pending-tag picker on a single row: the recording controls first at their full natural width, the tag picker directly after them; the group SHALL be horizontally centred in the transcript column. When the row is too narrow, the tag picker SHALL shrink and the recording controls SHALL NOT shrink, wrap, or be clipped. The live diarization status block SHALL keep its existing shape and position next to the recording indicator.

#### Scenario: Single row while recording
- **WHEN** a recording is in progress with pending tags selected
- **THEN** the pause/resume and stop controls, the recording indicator, the diarization status block, and the tag picker SHALL all appear on one row of the footer

#### Scenario: Row centred in the transcript column
- **WHEN** the footer row has more width than the controls and tag picker need
- **THEN** the group SHALL be horizontally centred under the transcript column, with no stretched empty space between the controls and the picker

#### Scenario: Narrow window
- **WHEN** the main content area is too narrow to show the recording controls and all pending tags at full width
- **THEN** the tag picker SHALL give up width first, and the recording controls SHALL remain fully visible and clickable

#### Scenario: Diarization status block unchanged
- **WHEN** a recording with online diarization is in progress
- **THEN** the two channel lines and the model-indicator row SHALL be shown immediately to the right of the animated recording indicator, as before the footer change

### Requirement: Compact single-line pending-tag picker
The pending-tag picker in the footer SHALL occupy a single line and SHALL NOT wrap onto further lines. It SHALL show no permanent text label. With an empty pending set, it SHALL show the tag icon with a "Tag this recording" placeholder. With a non-empty set, it SHALL show at most three pending tags as pills on one line, truncating long tag names, and SHALL collapse the remaining tags into one "+N" chip, where N is the number of hidden tags.

#### Scenario: Empty pending set
- **WHEN** no tags are pending
- **THEN** the picker SHALL show the tag icon and the placeholder "Tag this recording", and no pills

#### Scenario: Up to three pending tags
- **WHEN** the pending set is `{Work, Q3}`
- **THEN** both pills SHALL be shown on one line, each removable, with no "+N" chip and no placeholder

#### Scenario: More than three pending tags
- **WHEN** the pending set is `{Work, Q3, Client, Urgent, Demo}`
- **THEN** the first three pills SHALL be shown, followed by a "+2" chip, and the picker SHALL remain one line high

#### Scenario: Long tag names in a narrow row
- **WHEN** the row is narrow and the visible pending tags have long names
- **THEN** the tag names SHALL be truncated with an ellipsis and the picker SHALL remain one line high

#### Scenario: Full set reachable from the popover
- **WHEN** the user opens the picker's popover while some pending tags are collapsed into "+N"
- **THEN** every tag SHALL be listed with its pending state, and toggling or creating tags SHALL edit the pending set exactly as before

### Requirement: Stop-processing status shown in the footer
After recording stops, while transcripts are being finalized or the meeting is being saved, the footer SHALL remain in place and SHALL show the current status in place of the recording controls and the tag picker. No separate floating status element SHALL be shown over the transcript.

#### Scenario: Finalizing transcription
- **WHEN** the recording has stopped and transcripts are being finalized
- **THEN** the footer SHALL show a progress indicator with "Finalizing transcription..." in place of the recording controls, and nothing SHALL float over the transcript

#### Scenario: Saving transcript
- **WHEN** the meeting is being saved
- **THEN** the footer SHALL show a progress indicator with "Saving transcript..." in place of the recording controls

#### Scenario: Footer height stable across the stop sequence
- **WHEN** the session moves from recording to finalizing, saving, and back to idle
- **THEN** the footer SHALL stay visible throughout and the transcript area SHALL NOT jump to full height and back

### Requirement: Transient control feedback stays inside the footer
Transient feedback from the recording controls SHALL NOT be drawn over the transcript. This covers the pausing, resuming, and stopping indications and the device-error alert. The device-error alert SHALL NOT break the single row: the recording controls and tag picker SHALL stay on one row and remain usable while the alert is shown.

#### Scenario: Stopping indication
- **WHEN** the user clicks stop and the stop is in progress
- **THEN** the stopping indication SHALL be visible within the footer and SHALL NOT cover any transcript text

#### Scenario: Pausing indication
- **WHEN** the user clicks pause and the pause is in progress
- **THEN** the pausing indication SHALL be visible within the footer and SHALL NOT cover any transcript text

#### Scenario: Device error shown
- **WHEN** a device error alert is raised during recording
- **THEN** the alert SHALL be shown without covering transcript text, the controls and tag picker SHALL remain on one row, and the alert SHALL remain dismissible
