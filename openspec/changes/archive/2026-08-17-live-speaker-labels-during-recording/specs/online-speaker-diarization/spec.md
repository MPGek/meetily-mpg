## MODIFIED Requirements

### Requirement: Fast mode runs full streaming diarization during recording

The system SHALL use the polyvoice `StreamingPipeline` to perform segmentation, embedding extraction, and incremental speaker caching continuously during recording when Fast mode is selected.

#### Scenario: Streaming diarization processes audio chunks
- **WHEN** audio chunks arrive during recording in Fast mode
- **THEN** the system SHALL feed the VAD-detected 16 kHz speech chunks into the `StreamingPipeline`, which outputs speaker-labeled turns as they become available

#### Scenario: Speaker segments buffered internally
- **WHEN** the `StreamingPipeline` outputs a stable speaker turn during recording in Fast mode
- **THEN** the system SHALL buffer the turn internally for the final stop-time assignment pass

#### Scenario: Stable turns emitted live to frontend
- **WHEN** the `StreamingPipeline` outputs a stable speaker turn during recording in Fast mode
- **THEN** the system SHALL translate the turn to absolute recording time, emit it to the frontend via the `online-speaker-turn` event, and still buffer the turn for the final stop-time assignment pass
