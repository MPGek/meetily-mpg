## ADDED Requirements

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
