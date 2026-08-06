# database Specification — Delta

## ADDED Requirements

### Requirement: Speaker columns on transcripts
The `transcripts` table SHALL include `speaker TEXT` and `speaker_label TEXT` columns for storing speaker diarization results.

#### Scenario: Store speaker ID on transcript
- **WHEN** diarization assigns a speaker to a transcript segment
- **THEN** the `speaker` column SHALL contain the speaker ID (e.g., "SPEAKER_00")

#### Scenario: Store user-assigned speaker label
- **WHEN** user renames a speaker in the UI
- **THEN** the `speaker_label` column SHALL be updated with the human-readable name (e.g., "Alice")

#### Scenario: Legacy transcripts have null speaker
- **WHEN** existing transcripts without diarization are queried
- **THEN** `speaker` and `speaker_label` SHALL be NULL

### Requirement: Diarization metadata on meetings
The `meetings` table SHALL include `diarization_status TEXT` and `speaker_names TEXT` columns for tracking diarization state and speaker name mappings.

#### Scenario: Track diarization status
- **WHEN** diarization is running, succeeded, or failed for a meeting
- **THEN** `diarization_status` SHALL be set to "processing", "complete", or "failed" respectively

#### Scenario: Persist speaker name mappings
- **WHEN** user assigns names to speakers in a meeting
- **THEN** the `speaker_names` column SHALL store a JSON map of speaker IDs to labels (e.g., `{"SPEAKER_00": "Alice", "SPEAKER_01": "Bob"}`)
