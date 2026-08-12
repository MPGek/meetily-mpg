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
The system SHALL persist recording preferences (device selection, backend, folder path) across sessions and apply device changes to subsequent recordings without requiring an app restart.

#### Scenario: Save and restore device preference
- **WHEN** user selects a specific microphone and restarts the app
- **THEN** the previously selected device is automatically chosen

#### Scenario: Device selection applies to live recording
- **WHEN** user changes the system audio or microphone device on the settings page during the current session
- **THEN** the new selection SHALL be used by the next recording started from the main page without an app restart

### Requirement: Speaker diarization module
The audio engine SHALL include a `diarization` submodule that performs speaker diarization on recorded audio and assigns speaker labels to transcript segments.

#### Scenario: Diarization runs on stored audio
- **WHEN** `start_diarization` is called with a valid `meeting_id`
- **THEN** the diarization module SHALL decode the meeting's audio file, run the sherpa-onnx diarization pipeline, match speaker turns to transcript segments, and persist results to the database

#### Scenario: Diarization respects per-channel audio convention
- **WHEN** diarization processes a stereo audio file (left=mic, right=system)
- **THEN** it SHALL run diarization on the full audio, then override `speaker` to "SystemAudio" for all segments with `source_device="System"`

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

