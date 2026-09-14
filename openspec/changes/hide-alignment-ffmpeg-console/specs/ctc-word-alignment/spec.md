## ADDED Requirements

### Requirement: Windowless audio decoding for alignment repair
When the system extracts an audio span from a meeting's recorded audio for word-alignment refinement, it SHALL launch the external decoder without creating a visible console window on Windows.

#### Scenario: Offline re-diarization repair
- **WHEN** the user runs speaker re-analysis with word alignment enabled and audio spans are extracted from the meeting recording
- **THEN** no console windows appear during the refinement

#### Scenario: Stop-time finalize repair
- **WHEN** recording stops and segments lacking refined tokens are refined from the saved meeting audio
- **THEN** no console windows appear during the refinement
