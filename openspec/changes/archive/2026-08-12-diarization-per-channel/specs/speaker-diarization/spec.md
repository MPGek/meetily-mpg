## RENAMED Requirements

### Requirement: System audio segments overridden
FROM: System audio segments overridden
TO: System audio transcripts diarized from system channel

## MODIFIED Requirements

### Requirement: System audio transcripts diarized from system channel
The system SHALL assign speaker labels to system-source transcripts by diarizing the system channel independently, instead of overriding them with a single "SystemAudio" label.

#### Scenario: System transcripts get remote speaker IDs
- **WHEN** a transcript segment has `source_device="System"` and the system-channel diarization run produces a speaker turn overlapping its time range
- **THEN** the segment's `speaker` SHALL be set to the matching `SPEAKER_NN` ID

#### Scenario: System transcripts without a match
- **WHEN** a transcript segment has `source_device="System"` and no system-channel speaker turn overlaps its time range
- **THEN** the segment's `speaker` SHALL remain NULL

## ADDED Requirements

### Requirement: Per-channel offline diarization
The system SHALL process the microphone (left) and system (right) channels of a stereo recording independently during offline diarization, de-interleaving the decoded audio into two mono streams before segmentation.

#### Scenario: Stereo recording diarized per channel
- **WHEN** offline diarization runs on a stereo recording (2 channels, left=microphone, right=system)
- **THEN** the system SHALL de-interleave the decoded samples into a microphone stream and a system stream, resample each to 16kHz independently, and run the diarization pipeline once per channel, reusing a single diarizer instance loaded once

#### Scenario: Silent channel produces no speakers
- **WHEN** one channel of a stereo recording contains no speech
- **THEN** the system SHALL produce no speaker segments for that channel and its transcripts SHALL remain unlabeled rather than failing the whole run

### Requirement: Channel-specific speaker IDs
The system SHALL namespace speaker IDs by source channel so that cluster indices from the two independent diarization runs do not collide.

#### Scenario: Microphone speakers named
- **WHEN** the microphone-channel diarization run produces clusters 0..N
- **THEN** transcripts matched to those segments SHALL be assigned speaker IDs `MIC_SPEAKER_00` through `MIC_SPEAKER_NN`

#### Scenario: System speakers named
- **WHEN** the system-channel diarization run produces clusters 0..N
- **THEN** transcripts matched to those segments SHALL be assigned speaker IDs `SPEAKER_00` through `SPEAKER_NN`

#### Scenario: Per-source segment matching
- **WHEN** matching diarization segments to transcript time ranges
- **THEN** transcripts with `source_device="Microphone"` (or NULL) SHALL be matched against microphone-channel segments, and transcripts with `source_device="System"` SHALL be matched against system-channel segments

### Requirement: Mono recording fallback
The system SHALL treat mono recordings (or files without a distinct system channel) as a single remote-only source during offline diarization.

#### Scenario: Mono recording diarized as remote
- **WHEN** offline diarization runs on a mono recording
- **THEN** the system SHALL run the diarization pipeline once on the mono stream and assign all matched transcripts `SPEAKER_NN` IDs regardless of `source_device`

### Requirement: Prefixed speaker ID rendering
The transcript view SHALL render both speaker ID namespaces correctly and degrade gracefully for legacy labels.

#### Scenario: Mic speaker label display
- **WHEN** a transcript segment has speaker `MIC_SPEAKER_03`
- **THEN** the UI SHALL display "Mic Speaker 4" using the speaker color for index 3

#### Scenario: Remote speaker label display
- **WHEN** a transcript segment has speaker `SPEAKER_00`
- **THEN** the UI SHALL display "Speaker 1" using the speaker color for index 0

#### Scenario: Legacy SystemAudio label display
- **WHEN** a transcript segment retains the legacy speaker value `SystemAudio` from a prior diarization run
- **THEN** the UI SHALL display "System Audio" with a stable color instead of "Speaker NaN"
