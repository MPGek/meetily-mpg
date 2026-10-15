# Spec Delta

## Purpose

Decode a stored or imported audio file (for import, retranscription, and diarization fallback) into PCM samples. The sample rate, channel count, and duration reported for the file must match the audio actually decoded, not what the container claims.

## ADDED Requirements

### Requirement: Decoded sample rate comes from the decoded stream
When the decoder decodes an audio file, the sample rate it reports SHALL be the rate of the decoded audio. If the container declares a different rate, the declared rate SHALL be replaced and the correction logged.

#### Scenario: HE-AAC file declaring twice the decoded rate
- **WHEN** an HE-AAC (SBR) file whose container declares 48 kHz is decoded, and the decoder produces its AAC-LC core at 24 kHz
- **THEN** the decoded result SHALL report a sample rate of 24 kHz

#### Scenario: Container rate matches the decoded rate
- **WHEN** a file whose container rate equals the decoded rate (for example a 48 kHz WAV or AAC-LC file) is decoded
- **THEN** the reported sample rate SHALL equal that rate, unchanged

### Requirement: Decoded duration reflects the real length
The duration reported for a decoded audio file SHALL be computed from the number of decoded frames and the decoded sample rate. Audio converted to the 16 kHz mono transcription format SHALL keep that same duration.

#### Scenario: HE-AAC file keeps its real length
- **WHEN** a 5-second HE-AAC file whose container declares 48 kHz is decoded
- **THEN** the reported duration SHALL be about 5 seconds (not about 2.6 seconds), and its 16 kHz transcription-format samples SHALL also span about 5 seconds

#### Scenario: Imported meeting timeline matches the audio
- **WHEN** an HE-AAC file is imported and transcribed
- **THEN** transcript timestamps SHALL be on the same timeline as the original audio file's playback, so a segment spoken at minute 10 of the file is stamped at about minute 10
