## Purpose

Guarantees that recorded meeting audio is not silently lost when an incremental checkpoint or finalization step fails, and that any partial loss is clearly surfaced to the user instead of being reported as a successful save.

## ADDED Requirements

### Requirement: Audio save survives checkpoint encoding failures
The system SHALL continue saving audio after an individual checkpoint encode fails, isolating or discarding only the failed buffered segment, and SHALL NOT stall checkpointing by indefinitely retrying the same accumulating buffer.

#### Scenario: Single checkpoint encode fails mid-recording
- **WHEN** encoding of a checkpoint fails during an active recording
- **THEN** the saver SHALL discard the affected buffered segment, log the failure, and SHALL continue checkpointing the audio that follows

#### Scenario: Recording completes after a checkpoint failure
- **WHEN** a recording that had at least one failed checkpoint is stopped
- **THEN** the final audio file SHALL still be produced from the successfully written checkpoints and the outcome SHALL be reported as completed-with-partial-audio rather than silently lost

### Requirement: Encoder and merge operations are time-bounded
The system SHALL bound ffmpeg process operations (spawn, stdin write, wait for exit) with a timeout so a hung encoder process cannot stall audio saving or finalization indefinitely.

#### Scenario: Encoder process hangs during checkpointing
- **WHEN** a checkpoint encode operation exceeds its timeout
- **THEN** the operation SHALL be abandoned, the spawned process SHALL be terminated, and audio saving SHALL continue with the failure logged

#### Scenario: Merge hangs during finalization
- **WHEN** the checkpoint merge operation exceeds its timeout while producing the final audio file
- **THEN** finalization SHALL abandon the hung merge, terminate the process, and report the failure instead of hanging the stop flow

### Requirement: Saver failures are surfaced to the user
The system SHALL surface recording-save failures to the user instead of discarding recorded audio silently.

#### Scenario: Recording chunk cannot be delivered to the saver
- **WHEN** the audio pipeline attempts to deliver a mixed recording chunk and the saver channel is closed or unavailable
- **THEN** the delivery failure SHALL be logged and reported through the recording error surface rather than silently ignored

#### Scenario: Saver finalization fails
- **WHEN** recording finalization fails to merge or write the audio file
- **THEN** the user SHALL be informed that the audio portion of the meeting could not be saved, even if the transcript succeeded

### Requirement: Saved audio duration is validated against the session
The system SHALL compare the duration of the finalized audio file against the recorded session duration and SHALL report a significant mismatch at completion instead of indicating full success.

#### Scenario: Saved audio is shorter than the session
- **WHEN** a meeting completes and the merged audio duration is significantly shorter than the recorded session duration
- **THEN** the completion result SHALL surface a partial-audio warning to the user

#### Scenario: Saved audio matches the session
- **WHEN** a meeting completes and the merged audio duration approximately equals the recorded session duration
- **THEN** the completion result SHALL indicate full success without a partial-audio warning