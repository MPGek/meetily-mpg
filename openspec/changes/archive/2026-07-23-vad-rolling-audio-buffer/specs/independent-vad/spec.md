# independent-vad Specification Delta

## MODIFIED Requirements

### Requirement: Per-source VAD processing
The system SHALL run independent Voice Activity Detection on microphone and system audio streams, using separate VAD processor instances with identical initial configuration. Each VAD processor SHALL maintain a rolling audio buffer that stores the most recent processed windows and uses it to backfill speech onset audio when speech is detected.

#### Scenario: Microphone speech detected
- **WHEN** audio is captured from the microphone device and contains human speech above VAD threshold
- **THEN** the microphone VAD processor SHALL emit a speech segment tagged with `DeviceType::Microphone`
- **AND** the segment SHALL include audio from the rolling buffer prepended to the detection window, recovering the speech onset

#### Scenario: System audio speech detected
- **WHEN** audio is captured from the system audio device and contains human speech above VAD threshold
- **THEN** the system audio VAD processor SHALL emit a speech segment tagged with `DeviceType::System`
- **AND** the segment SHALL include audio from the rolling buffer prepended to the detection window, recovering the speech onset

#### Scenario: Simultaneous speech on both sources
- **WHEN** both microphone and system audio contain speech simultaneously
- **THEN** both VAD processors SHALL independently emit speech segments with their respective device type tags
- **AND** each segment SHALL include audio from its own rolling buffer

#### Scenario: Silence on one source
- **WHEN** one source has speech while the other is silent
- **THEN** only the VAD processor for the active source SHALL emit speech segments; the silent source SHALL produce no output
