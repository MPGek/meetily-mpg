# live-segment-merging Specification

## Purpose
Real-time merging of adjacent VAD segments in the live audio pipeline, producing coherent multi-utterance chunks for Whisper transcription instead of isolated fragments.

## Requirements

### Requirement: Live VAD segment accumulation
The system SHALL accumulate VAD segments in the live pipeline and merge adjacent segments whose inter-segment gap is less than 500ms before dispatching to the transcription worker.

#### Scenario: Adjacent segments within 500ms gap are merged
- **WHEN** VAD emits segment A ending at time T₁ and segment B starting at time T₂, where T₂ − T₁ < 500ms
- **THEN** the pipeline SHALL combine A and B into a single audio chunk containing A.samples + gap_silence + B.samples
- **AND** the combined chunk SHALL be sent to transcription as one unit

#### Scenario: Distant segments with gap ≥ 500ms are kept separate
- **WHEN** VAD emits segment A ending at time T₁ and segment B starting at time T₂, where T₂ − T₁ ≥ 500ms
- **THEN** segment A SHALL be dispatched to transcription immediately
- **AND** segment B SHALL begin a new accumulation window

#### Scenario: Merged segment exceeds 25 seconds
- **WHEN** merged live segments would exceed 25 seconds of audio
- **THEN** the pipeline SHALL split at the largest silence gap and dispatch the first part before continuing accumulation

### Requirement: No live-mode segment merger duplicates
The live pipeline's segment merging SHALL reuse `merge_segments` from `audio/vad.rs` with `max_gap_ms=500` and `max_duration_samples=25*16000` instead of duplicating merge logic.

#### Scenario: Pipeline uses shared merger
- **WHEN** the live pipeline processes VAD segments
- **THEN** it SHALL call `merge_segments(&segments, 500.0, 25*16000)` to produce processable chunks
