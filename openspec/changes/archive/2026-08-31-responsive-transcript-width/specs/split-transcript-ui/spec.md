## ADDED Requirements

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
