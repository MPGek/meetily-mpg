## ADDED Requirements

### Requirement: Online diarization embedding channel
The system SHALL provide a parallel audio routing channel (`embedding_sender`) alongside the existing transcription channel in the audio pipeline, allowing online diarization to consume VAD-filtered audio chunks independently.

#### Scenario: Pipeline creates embedding channel when diarization enabled
- **WHEN** recording starts with online diarization mode set to "Fast" or "Efficient"
- **THEN** the `AudioPipelineManager` SHALL create an `mpsc::UnboundedSender<AudioChunk>` for embeddings and spawn an `OnlineDiarizationProcessor` as the consumer

#### Scenario: Pipeline skips embedding channel when diarization disabled
- **WHEN** recording starts with online diarization mode set to "Off"
- **THEN** the `AudioPipelineManager` SHALL NOT create an embedding channel or spawn an online diarization processor

#### Scenario: VAD-filtered audio sent to embedding channel
- **WHEN** the pipeline dispatches a VAD-merged speech segment to the transcription sender
- **THEN** the system SHALL also send the same audio chunk to the embedding sender if it exists

### Requirement: Online diarization cleanup on recording stop
The system SHALL properly terminate the online diarization processor and free its resources when recording stops.

#### Scenario: Efficient mode buffer freed on stop
- **WHEN** recording stops in Efficient mode
- **THEN** the system SHALL trigger clustering on buffered embeddings, update transcripts, then drop the embedding buffer to free memory

#### Scenario: Fast mode processor stopped on recording stop
- **WHEN** recording stops in Fast mode
- **THEN** the system SHALL signal the polyvoice `StreamingPipeline` to flush remaining turns and shut down
