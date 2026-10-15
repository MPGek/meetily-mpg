# Spec Delta

## ADDED Requirements

### Requirement: Live transcripts are not filtered by engine confidence
During a live recording, the transcription worker SHALL emit every transcript whose text is not empty or whitespace-only, whatever confidence value the transcription engine reports. The confidence value SHALL be used only for logging and SHALL NOT decide whether a live transcript is emitted.

#### Scenario: Short reply is emitted live
- **WHEN** the transcription engine returns a short non-empty transcript such as "Yes" or "Да" for a live speech segment, with a reported confidence below 0.3
- **THEN** the worker SHALL emit a transcript update for it, as import and retranscription of the same audio already do

#### Scenario: Empty result is not emitted
- **WHEN** the transcription engine returns an empty or whitespace-only transcript for a live speech segment
- **THEN** the worker SHALL NOT emit a transcript update for that segment

#### Scenario: Same rule for every engine
- **WHEN** the live recording uses Whisper, Parakeet, or a provider engine
- **THEN** the emit decision SHALL be the same non-empty-text rule for all of them
