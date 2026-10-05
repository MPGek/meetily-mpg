# Spec Delta

## MODIFIED Requirements

### Requirement: Live auto-follow control
The live transcription view SHALL follow the bottom with new segments only while the view is pinned to the bottom, SHALL never yank the view down after the user scrolls up, and SHALL offer a visible control to return to the live bottom.

#### Scenario: Scrolled-up view stays put on new segments
- **WHEN** the user has scrolled up in the live transcript view and a new transcript segment arrives
- **THEN** the view SHALL NOT move, including when the user scrolls up within the auto-scroll delay window after the segment arrived

#### Scenario: Scroll-to-bottom button appears when not pinned
- **WHEN** the live transcript view is not pinned to the bottom
- **THEN** a circular overlay button with a downward arrow SHALL be shown at the bottom-right of the transcript panel, just above the docked recording footer, and SHALL NOT overlap the footer

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
