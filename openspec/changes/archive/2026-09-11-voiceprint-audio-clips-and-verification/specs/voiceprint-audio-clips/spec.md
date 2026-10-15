## Purpose

Stores a short self-contained voice sample alongside every speaker voiceprint embedding so the voice can be validated by listening without depending on the meeting's original audio file.

## ADDED Requirements

### Requirement: Opus mono clip stored per voiceprint row
Each `speaker_embeddings` row SHALL carry a self-contained voice clip of its source segment in addition to the embedding: Opus mono audio resampled to 16 kHz at approximately 24 kbps (voip application), sliced from the same capture channel the embedding came from, capped at approximately 15 seconds per row. New rows written by any diarization or enrollment path SHALL include the clip; rows written before this capability SHALL remain valid with no clip.

#### Scenario: New prototype carries its own audio
- **WHEN** diarization enrolls a prototype from a mic-channel segment
- **THEN** the row SHALL contain an Opus mono clip of that segment from the mic channel, playable without the meeting audio file

#### Scenario: Legacy rows remain valid without clips
- **WHEN** a voiceprint row written before audio clips existed is listed
- **THEN** it SHALL be shown with an "audio unavailable" state rather than an error, and all other row behavior SHALL be unchanged

#### Scenario: Channel fidelity
- **WHEN** a clip is stored for a system-channel embedding
- **THEN** the clip audio SHALL come from the system channel, not the mic channel

### Requirement: Blob playback without the meeting file
For any voiceprint row with a stored clip, the system SHALL play the clip directly from the stored bytes via a dedicated audio path that does not read the meeting's audio file and does not seek by provenance timecodes. Playback SHALL stream without transferring the full meeting recording over IPC. Rows without a stored clip SHALL fall back to the legacy meeting-file seek path where available, and SHALL show playback disabled otherwise.

#### Scenario: Clip plays from stored bytes
- **WHEN** the user activates play on a row with a stored clip whose meeting audio file is missing
- **THEN** playback SHALL still start and play the stored voice sample in full

#### Scenario: No timecode seeking for stored clips
- **WHEN** a stored clip plays
- **THEN** playback SHALL play the clip from its start to its end with no seek offset derived from provenance timecodes

#### Scenario: Streaming without full-file transfer
- **WHEN** a stored clip plays
- **THEN** only the clip bytes SHALL cross the playback path, never the full meeting recording
