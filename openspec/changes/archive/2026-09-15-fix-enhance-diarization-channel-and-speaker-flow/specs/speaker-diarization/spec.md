## MODIFIED Requirements

### Requirement: Per-channel offline diarization
The system SHALL process the microphone (left) and system (right) channels of a stereo recording independently during offline diarization, splitting the audio into two mono streams before segmentation. The decision to split channels SHALL be based on the channel layout of the actually decoded audio, not on container/header metadata alone; when the decoded audio has two channels the system SHALL treat the recording as stereo and SHALL NOT downmix it into a single stream, even when the container reports missing or conflicting channel information.

#### Scenario: Stereo recording diarized per channel
- **WHEN** offline diarization runs on a stereo recording (2 channels, left=microphone, right=system)
- **THEN** the system SHALL produce a 16kHz microphone stream and a 16kHz system stream, resample each to 16kHz independently, and run the diarization pipeline once per channel, reusing a single diarizer instance loaded once

#### Scenario: Silent channel produces no speakers
- **WHEN** one channel of a stereo recording contains no speech
- **THEN** the system SHALL produce no speaker segments for that channel and its transcripts SHALL remain unlabeled rather than failing the whole run

#### Scenario: Container metadata lacks channel count
- **WHEN** the container/header metadata does not expose a channel count but the decoded audio has two channels
- **THEN** the system SHALL diarize the microphone and system channels independently and SHALL NOT mix them into a single stream

### Requirement: Mono recording fallback
The system SHALL treat mono recordings (or files whose decoded audio has a single channel) as a single remote-only source during offline diarization. The system SHALL take the mono path only when the decoded audio genuinely has one channel; missing or unknown container metadata SHALL NOT by itself force a stereo recording onto the mono path.

#### Scenario: Mono recording diarized as remote
- **WHEN** offline diarization runs on a mono recording
- **THEN** the system SHALL run the diarization pipeline once on the mono stream and assign all matched transcripts `SPEAKER_NN` IDs regardless of `source_device`

#### Scenario: Unknown metadata on a stereo file
- **WHEN** offline diarization runs on a file whose decoded audio has two channels but whose metadata reports one channel or is unknown
- **THEN** the system SHALL follow the per-channel stereo path and SHALL NOT use the mono fallback
