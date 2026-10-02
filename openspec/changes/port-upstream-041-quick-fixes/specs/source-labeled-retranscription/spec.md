# Spec Delta

## ADDED Requirements

### Requirement: Retranscription records the decoded duration
When retranscription completes, the meeting's metadata SHALL record the audio duration decoded during that retranscription, replacing any previously stored duration. All other existing metadata fields SHALL be preserved, apart from those that retranscription already resets.

#### Scenario: Stale duration is corrected
- **WHEN** a meeting's existing metadata stores a duration that differs from the duration decoded during retranscription (for example, half the real length from an earlier HE-AAC import)
- **THEN** after retranscription completes, the stored duration SHALL equal the newly decoded duration

#### Scenario: Unrelated metadata survives
- **WHEN** retranscription updates an existing metadata file that contains fields such as the meeting id, audio file name, or summary language
- **THEN** those fields SHALL keep their previous values

#### Scenario: No existing metadata
- **WHEN** retranscription completes for a meeting folder without a metadata file
- **THEN** the newly written metadata SHALL contain the decoded duration
