# online-speaker-diarization Delta Spec

## ADDED Requirements

### Requirement: Stop-time finalize operates on token-bearing segments
Live transcript updates SHALL propagate the transcription engine's word-level tokens to the buffered segments used by stop-time finalize, and completed blocks SHALL be refined by live word-level alignment during recording (per the ctc-word-alignment capability) so finalize operates on word-true timestamps it already holds. At recording stop, pending live-alignment work SHALL be given a bounded drain before finalize; buffered segments still lacking refined tokens SHALL be refined by the repair path (when enabled and available) before the N-way speaker split; buffered segments without tokens SHALL retain segment-level overlap matching.

#### Scenario: Live tokens reach stop-time finalize
- **WHEN** a recording with online diarization stops and buffered segments carry word tokens from the transcription engine
- **THEN** the stop-time assignment SHALL attribute tokens to covering speaker turns and split a buffered segment spanning multiple speakers into one transcript row per contiguous speaker block

#### Scenario: Finalize splits on live-refined tokens
- **WHEN** recording stops with word alignment enabled and buffered segments already carrying refined tokens
- **THEN** the stop-time assignment SHALL split on the refined timestamps without re-aligning those segments

#### Scenario: Tokenless segments still finalize by overlap
- **WHEN** recording stops with a buffered segment that carries no word tokens
- **THEN** that segment SHALL be labeled by maximum temporal overlap exactly as before this change
