# speaker-diarization Delta Spec

## ADDED Requirements

### Requirement: Offline diarization operates on word-level tokens
Transcript segments produced during recording SHALL carry the word-level tokens emitted by the transcription engine — refined by live word-level alignment when available — and those tokens SHALL be persisted with the saved transcript rows so that offline diarization of a meeting can operate at word granularity. When a segment carries tokens, offline speaker attribution SHALL refine timestamps via word-level alignment for segments that do not already carry refined tokens (when enabled and available) and then attribute each token to the covering speaker turn, splitting cross-speaker segments at token boundaries. Segments without tokens SHALL keep segment-level overlap matching with no row splitting.

#### Scenario: Recorded meeting persists word tokens
- **WHEN** a recording with transcription saves its transcript to the meeting
- **THEN** each saved transcript row SHALL include the word-level tokens (text + start/end) produced for that segment

#### Scenario: Re-diarization of a saved meeting splits on tokens
- **WHEN** offline diarization runs on a meeting whose transcript rows carry word tokens
- **THEN** speaker attribution SHALL operate per token, and a row whose tokens span two or more speakers SHALL be split into one row per contiguous speaker block

#### Scenario: Legacy rows without tokens
- **WHEN** offline diarization runs on transcript rows that have no stored word tokens
- **THEN** those rows SHALL be labeled by segment-level overlap matching only and SHALL NOT be split
