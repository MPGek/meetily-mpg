# Spec Delta

## Purpose

Guarantees that the recording engine's shared locks, background threads, and internal channels stay correct and responsive under concurrent access, isolated worker failures, and a slow or stalled consumer, instead of silently blocking commands, permanently poisoning shared state, or growing memory without bound.

## ADDED Requirements

### Requirement: Recording commands stay responsive during a device reconnect

The system SHALL NOT hold the global recording-manager lock for the duration of an in-progress device reconnect attempt. A command that needs the recording manager for an unrelated operation SHALL be able to acquire it without waiting for a concurrent reconnect attempt to finish.

#### Scenario: Stop completes while a reconnect is in progress
- **WHEN** `stop_recording` is invoked while a device reconnect attempt is in progress
- **THEN** `stop_recording` SHALL acquire the recording manager and proceed without blocking for the remaining duration of the reconnect attempt

#### Scenario: Reconnect attempt still completes and updates state
- **WHEN** a device reconnect attempt finishes after being invoked concurrently with another recording command
- **THEN** its success or failure SHALL still be reflected in the recording manager's state once both operations have completed

### Requirement: A poisoned shared lock does not permanently disable recording commands

If a thread panics while holding a lock on a shared recording-engine state, the system SHALL recover the lock's contents for the next acquisition rather than making every subsequent acquisition of that same lock fail.

#### Scenario: Panic while holding a shared lock
- **WHEN** a thread panics while holding the lock on a shared recording-engine state (for example, the global recording manager or a shared session-status map)
- **THEN** a subsequent operation that acquires the same lock SHALL succeed and see the state as it was left, rather than failing because the lock was poisoned

### Requirement: Background worker threads accept an explicit stop request

A long-running background worker thread owned by the recording/diarization engine (including the ffmpeg diagnostic-output reader and the memory sampler) SHALL support being asked to stop independently of the process exiting, and the requester SHALL be able to observe that the thread has stopped within a bounded time.

#### Scenario: Explicit stop is honored
- **WHEN** a background worker thread is signaled to stop while it is still running its loop
- **THEN** the thread SHALL observe the signal and exit its loop without requiring the owning value to be dropped or the process to exit

### Requirement: Audio delivery does not grow memory unboundedly when a consumer stalls

A channel that carries captured or transcription-bound audio data for an active recording SHALL have a bounded capacity, and the system SHALL define and apply a specific behavior (either applying backpressure to the producer or dropping data with a logged warning) when that capacity is reached, rather than buffering an unbounded amount of audio in memory.

#### Scenario: Downstream audio consumer falls behind
- **WHEN** the consumer of an audio-carrying channel (the mixing pipeline, the transcription stage, the online-diarization embedding stage, or the recording accumulator) falls behind the producer during an active recording
- **THEN** the channel SHALL reach its bounded capacity instead of growing without limit, and the defined drop-or-backpressure behavior SHALL take effect

#### Scenario: Recording continues after a bounded channel applies backpressure or drops
- **WHEN** a bounded audio-carrying channel reaches capacity and applies its defined behavior
- **THEN** the recording session SHALL continue running rather than the affected command failing outright
