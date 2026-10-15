## MODIFIED Requirements

### Requirement: Live VAD segment accumulation
The system SHALL accumulate VAD segments in the live pipeline and merge adjacent segments whose inter-segment gap is less than 500ms before dispatching to the transcription worker.

#### Scenario: Adjacent segments within 500ms gap are merged
- **WHEN** VAD emits segment A ending at time T₁ and segment B starting at time T₂, where T₂ − T₁ < 500ms
- **THEN** the pipeline SHALL combine A and B into a single audio chunk containing A.samples + gap_silence + B.samples
- **AND** the combined chunk SHALL be sent to transcription as one unit

#### Scenario: Distant segments with gap ≥ 500ms are kept separate
- **WHEN** VAD emits segment A ending at time T₁ and segment B starting at time T₂, where T₂ − T₁ ≥ 500ms
- **THEN** segment A SHALL be dispatched to transcription no later than the first VAD dispatch window after T₁ + 500ms + the pre-speech padding (150 ms) + one VAD frame (32 ms), without waiting for segment B to end
- **AND** segment B SHALL begin a new accumulation window

#### Scenario: Speaker stops talking
- **WHEN** the last pending segment on a channel ends at time T₁ and the channel carries no further speech
- **THEN** the pending speech SHALL be dispatched to transcription no later than the first VAD dispatch window after T₁ + 500ms + the pre-speech padding (150 ms) + one VAD frame (32 ms)
- **AND** it SHALL NOT wait for later speech on that channel or for recording stop

#### Scenario: Silence shorter than the gap
- **WHEN** the last pending segment ends at time T₁ and new speech on the same channel starts before T₁ + 500ms
- **THEN** the pending speech SHALL NOT be dispatched on account of that silence
- **AND** the new speech SHALL be merged with it as before

#### Scenario: Chunk boundaries unchanged
- **WHEN** the same audio is processed with dispatch on silence and with dispatch only when the next segment arrives
- **THEN** both SHALL produce the same set of chunks with the same audio; only the time at which each chunk is dispatched differs

#### Scenario: Merged segment exceeds 25 seconds
- **WHEN** merged live segments would exceed 25 seconds of audio
- **THEN** the pipeline SHALL split at the largest silence gap and dispatch the first part before continuing accumulation
