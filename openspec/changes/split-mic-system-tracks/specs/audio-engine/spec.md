## MODIFIED Requirements

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

## REMOVED Requirements

### Requirement: Audio mixing
**Reason**: Audio mixing (summing mic and system into mono) is replaced by stereo interleaving. Each source preserves its own channel. The `ProfessionalAudioMixer` struct and `mix_window` method are removed.

**Migration**: No user action required. Saved recordings will be stereo instead of mono. Consumers that need mono can downmix by averaging left and right channels.
