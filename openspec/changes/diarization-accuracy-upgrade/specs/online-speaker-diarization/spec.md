## MODIFIED Requirements

### Requirement: Efficient mode extracts speaker embeddings during recording
The system SHALL extract speaker embeddings from VAD-detected speech segments during recording when Efficient mode is selected, buffering them in memory without performing clustering. The embedder SHALL be model-aware: when the enhanced model set is installed, embedding extraction SHALL use the TitaNet-Large model; otherwise it SHALL use the polyvoice `ResNet34Adapter`. The selected model family SHALL be recorded with the buffered embeddings so that the stop-time clustering and any enrollment use embeddings from a single family.

#### Scenario: Embedding extracted from speech segment
- **WHEN** the VAD detects a speech segment of at least 200ms duration during recording in Efficient mode
- **THEN** the system SHALL resample the segment to 16 kHz if needed and extract a speaker embedding vector using the model-aware embedder (TitaNet-Large when enhanced models are installed, ResNet34 otherwise), storing it with the segment's start and end timestamps and its model family

#### Scenario: Silence periods skipped
- **WHEN** the VAD detects silence (no speech) for more than 500ms during recording in Efficient mode
- **THEN** the system SHALL NOT extract embeddings for the silence period

#### Scenario: Enhanced embeddings used when installed
- **WHEN** recording in Efficient mode with the enhanced model set installed
- **THEN** embedded vectors SHALL be 192-dimensional TitaNet embeddings and clustering at stop SHALL apply the TitaNet family threshold rather than the ResNet34 0.45 threshold

## ADDED Requirements

### Requirement: Stop-time assignment uses token-level refinement
At recording stop, when buffered transcript segments carry token timestamps, speaker matching SHALL refine ownership at token granularity before writing labels: tokens SHALL be attributed to the speaker of the covering turn, and a transcript segment spanning a speaker change SHALL be split into separate transcript rows at the boundary token. Segment overlap matching SHALL be used for segments without token timestamps. Results SHALL be written through the same `update_transcript_speaker` path as offline diarization.

#### Scenario: Cross-speaker chunk split at stop
- **WHEN** recording stops and a buffered transcript chunk with token timestamps spans `N≥2` distinct speakers detected by the online diarization turns
- **THEN** the chunk SHALL be stored as `N` transcript rows, one per contiguous speaker block (each boundary requires ≥2 contiguous tokens of the new speaker), each labeled with its block's speaker and timestamps adjusted to that block's token span, via the same transcript write path used for segment-level matching

#### Scenario: Single-speaker chunk labeled normally
- **WHEN** recording stops and a buffered transcript chunk with token timestamps is covered by a single speaker's turns
- **THEN** the chunk SHALL be labeled with that speaker without splitting

#### Scenario: No token timestamps falls back to overlap
- **WHEN** recording stops and a buffered transcript chunk has no token timestamps
- **THEN** the chunk SHALL be labeled by maximum temporal overlap with the diarization turns, exactly as before this change