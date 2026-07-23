# independent-vad Specification Delta

## MODIFIED Requirements

### Requirement: Per-source VAD processing
The system SHALL run independent Voice Activity Detection on microphone and system audio streams, using separate VAD processor instances configured via `VadConfig::live()`.

#### Scenario: Microphone speech detected
- **WHEN** audio is captured from the microphone device and contains human speech above VAD threshold
- **THEN** the microphone VAD processor SHALL emit a speech segment tagged with `DeviceType::Microphone`

#### Scenario: System audio speech detected
- **WHEN** audio is captured from the system audio device and contains human speech above VAD threshold
- **THEN** the system audio VAD processor SHALL emit a speech segment tagged with `DeviceType::System`

#### Scenario: Simultaneous speech on both sources
- **WHEN** both microphone and system audio contain speech simultaneously
- **THEN** both VAD processors SHALL independently emit speech segments with their respective device type tags

#### Scenario: Silence on one source
- **WHEN** one source has speech while the other is silent
- **THEN** only the VAD processor for the active source SHALL emit speech segments; the silent source SHALL produce no output
