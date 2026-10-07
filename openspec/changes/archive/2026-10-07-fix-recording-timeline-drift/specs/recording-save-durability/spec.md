# Spec Delta

## MODIFIED Requirements

### Requirement: Encoder and merge operations are time-bounded
The system SHALL bound ffmpeg process operations (spawn, stdin write, wait for exit) with a timeout so a hung encoder process cannot stall audio saving or finalization indefinitely. The finalize timeout SHALL grow with the length of the recording, so a long recording that is still encoding normally is not abandoned.

#### Scenario: Encoder process hangs during checkpointing
- **WHEN** a checkpoint encode operation exceeds its timeout
- **THEN** the operation SHALL be abandoned, the spawned process SHALL be terminated, and audio saving SHALL continue with the failure logged

#### Scenario: Merge hangs during finalization
- **WHEN** the checkpoint merge operation exceeds its timeout while producing the final audio file
- **THEN** finalization SHALL abandon the hung merge, terminate the process, and report the failure instead of hanging the stop flow

#### Scenario: Long recording finalizes
- **WHEN** a 3-hour recording is finalized on a machine that encodes audio at least 10 times faster than real time
- **THEN** finalization SHALL complete and produce the audio file without any merge or save timeout firing
