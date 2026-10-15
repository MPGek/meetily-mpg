## MODIFIED Requirements

### Requirement: Audio recording start/stop
The system SHALL support starting and stopping audio recording with configurable mic and system capture devices, resolving any unspecified device to the system default instead of disabling its capture.

#### Scenario: Start recording with default devices
- **WHEN** user clicks "Start Recording" without specifying devices
- **THEN** system starts recording using the default input device and the default output device, and saves to the meeting folder

#### Scenario: Start recording with only a microphone selected
- **WHEN** user starts recording with a microphone device selected but no system audio device
- **THEN** system SHALL capture system audio from the default output device instead of skipping system audio capture

#### Scenario: Start recording with only a system audio device selected
- **WHEN** user starts recording with a system audio device selected but no microphone device
- **THEN** system SHALL capture microphone audio from the default input device

#### Scenario: Stop recording gracefully
- **WHEN** user clicks "Stop Recording" during an active session
- **THEN** system stops capture, saves audio file, and releases resources

### Requirement: Recording preferences persistence
The system SHALL persist recording preferences (device selection, backend, folder path) across sessions and apply device changes to subsequent recordings without requiring an app restart.

#### Scenario: Save and restore device preference
- **WHEN** user selects a specific microphone and restarts the app
- **THEN** the previously selected device is automatically chosen

#### Scenario: Device selection applies to live recording
- **WHEN** user changes the system audio or microphone device on the settings page during the current session
- **THEN** the new selection SHALL be used by the next recording started from the main page without an app restart
