# Spec Delta

## ADDED Requirements

### Requirement: Merging checkpoints keeps the recorded sample count
When checkpoints are merged into the final recording, the system SHALL produce audio whose length and timing equal the concatenated checkpoint audio. It SHALL NOT insert encoder delay or padding at checkpoint boundaries. Over a whole recording, the merged file SHALL differ from the checkpoint audio by at most one encoder frame (about 21 ms).

#### Scenario: One-hour recording
- **WHEN** a 60-minute recording made of about 120 checkpoints is finalized
- **THEN** the merged file's duration SHALL be within 25 ms of the audio delivered to the saver, and a sound delivered at 59:00 SHALL play at 59:00 within 25 ms

#### Scenario: Sound right after a checkpoint boundary
- **WHEN** a short sound is recorded just after the boundary between two checkpoints
- **THEN** it SHALL play at the time it was recorded, with no added silence or delay at the boundary

### Requirement: Crash recovery merges checkpoints the same way
When audio is recovered from the checkpoints of an interrupted recording, the recovered file SHALL keep the recorded sample count by the same rule as a normal finalize. Checkpoint folders written by earlier versions SHALL remain recoverable.

#### Scenario: Recovering an interrupted recording
- **WHEN** the app crashes during a recording and the user recovers its audio
- **THEN** the recovered file SHALL keep the checkpointed audio's timing, with no added delay at checkpoint boundaries

#### Scenario: Recovering a folder left by an earlier version
- **WHEN** the user recovers a recording whose checkpoints were written by an earlier version of the app
- **THEN** recovery SHALL still produce an audio file from those checkpoints
