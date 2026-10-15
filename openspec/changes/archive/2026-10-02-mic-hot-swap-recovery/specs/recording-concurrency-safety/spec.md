# Spec Delta

## MODIFIED Requirements

### Requirement: Recording commands stay responsive during a device reconnect

The system SHALL NOT hold the global recording-manager lock while a mid-recording microphone switch is in progress. This covers tearing down the lost device's stream, resolving the replacement device and opening its stream. A command that needs the recording manager for an unrelated operation, including Stop, SHALL be able to acquire it without waiting for a concurrent switch to finish.

#### Scenario: Stop completes while a reconnect is in progress
- **WHEN** `stop_recording` is invoked while a microphone switch is in progress
- **THEN** `stop_recording` SHALL acquire the recording manager and proceed without blocking for the remaining duration of the switch, and the switch SHALL be discarded instead of applied to the stopped session

#### Scenario: Reconnect attempt still completes and updates state
- **WHEN** a microphone switch finishes while other recording commands (for example pause, resume or a recording-state query) ran concurrently, and the session was not stopped
- **THEN** the session's recorded microphone device and active microphone stream SHALL reflect the replacement device once both operations have completed
