# audio-engine Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: Audio device discovery
The system SHALL list available audio input devices with their names, IDs, and types (microphone or system).

#### Scenario: List audio devices on startup
- **WHEN** user opens the app or clicks "Get Devices" button
- **THEN** system returns a list of detected audio devices with name, ID, and type

### Requirement: Audio recording start/stop
The system SHALL support starting and stopping audio recording with configurable mic and system capture devices.

#### Scenario: Start recording with default devices
- **WHEN** user clicks "Start Recording" without specifying devices
- **THEN** system starts recording using default input device and saves to meeting folder

#### Scenario: Stop recording gracefully
- **WHEN** user clicks "Stop Recording" during an active session
- **THEN** system stops capture, saves audio file, and releases resources

### Requirement: Dual-channel audio capture
The system SHALL simultaneously capture microphone and system audio on supported platforms, keeping each source on a dedicated stereo channel (microphone on left, system audio on right) without mixing.

#### Scenario: Capture both mic and system audio on macOS
- **WHEN** recording starts with mic_device_name and system_device_name specified
- **THEN** system opens two independent audio streams and processes them as separate stereo channel contributions

#### Scenario: Microphone mapped to left channel
- **WHEN** microphone audio is captured by AudioCapture
- **THEN** audio data SHALL be interleaved as stereo with microphone samples on the left channel and zeros on the right channel

#### Scenario: System audio mapped to right channel
- **WHEN** system audio is captured by AudioCapture
- **THEN** audio data SHALL be interleaved as stereo with zeros on the left channel and system audio samples on the right channel

### Requirement: Audio level monitoring
The system SHALL monitor and report audio levels (RMS) for active capture devices.

#### Scenario: Monitor audio levels during recording
- **WHEN** recording is in progress
- **THEN** system emits periodic audio level updates via Tauri events

### Requirement: Device detection and reconnection
The system SHALL detect Bluetooth/AirPods disconnect/reconnect events and attempt automatic reconnection.

#### Scenario: Detect AirPods disconnection
- **WHEN** user removes AirPods during recording
- **THEN** system detects device change within 2 seconds and shows notification

### Requirement: Audio backend selection
The system SHALL support multiple audio capture backends (CoreAudio on macOS, WASAPI on Windows, PulseAudio/ALSA on Linux).

#### Scenario: Select CoreAudio backend on macOS
- **WHEN** user selects "CoreAudio" as audio backend in settings
- **THEN** system uses macOS CoreAudio for all subsequent recordings

### Requirement: Recording preferences persistence
The system SHALL persist recording preferences (device selection, backend, folder path) across sessions.

#### Scenario: Save and restore device preference
- **WHEN** user selects a specific microphone and restarts the app
- **THEN** the previously selected device is automatically chosen

