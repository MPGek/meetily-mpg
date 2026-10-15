## MODIFIED Requirements

### Requirement: Deterministic default color from fixed palette with manual override

The system SHALL assign every new tag a default color selected deterministically from a fixed palette of at least 30 distinct colors, and SHALL allow the color to be changed afterwards without renaming the tag.

#### Scenario: Default color assigned

- **WHEN** user creates a tag without specifying a color
- **THEN** the system assigns a palette color deterministically so the same tag name always resolves to the same default color on a fresh database

#### Scenario: Palette cycles

- **WHEN** more tags are created than there are palette entries
- **THEN** the system cycles through the palette rather than failing or reusing only one color

#### Scenario: Manual color override

- **WHEN** user changes the color of an existing tag
- **THEN** the system persists the new color and all meetings showing that tag display the new color

#### Scenario: Palette offers at least 30 distinct colors

- **WHEN** the tag color palette is available to the tag editor
- **THEN** it exposes at least 30 distinct color keys, and every key has a distinct chip background

#### Scenario: Manual color cycle traverses the whole palette

- **WHEN** user repeatedly cycles the color of a tag
- **THEN** the color visits every palette entry before repeating
