# Spec Delta

## MODIFIED Requirements

### Requirement: Audio recording start/stop
The system SHALL support starting and stopping audio recording with configurable mic and system capture devices, resolving any unspecified device to the system default instead of disabling its capture. A failure to initialize the audio processing pipeline (including voice activity detection) SHALL fail the start request with an error that states the cause, and SHALL NOT terminate the application.

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

#### Scenario: Voice activity detection cannot be initialized
- **WHEN** the user starts a recording and the microphone or system-audio voice activity detector fails to initialize
- **THEN** the start request SHALL return an error naming the failed component and its cause, the application SHALL keep running, and the recording state SHALL be left as not recording, so a later start attempt is possible

#### Scenario: Start failure shown to the user
- **WHEN** a recording start initiated from the home page or the sidebar fails
- **THEN** the message shown to the user SHALL include the error text returned by the backend, instead of only telling the user to check the console
