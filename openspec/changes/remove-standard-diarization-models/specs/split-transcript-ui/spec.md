# Delta spec: split-transcript-ui

## MODIFIED Requirements

### Requirement: Re-analyze Speakers button
The meeting details page SHALL include a "Re-analyze Speakers" button that triggers diarization on the current meeting.

#### Scenario: Button visible when models are available
- **WHEN** the bundled enhanced diarization models are available and a meeting has saved audio
- **THEN** a "Re-analyze Speakers" button SHALL be visible in the meeting detail view

#### Scenario: Button disabled during processing
- **WHEN** diarization is running for this meeting (`diarization_status="processing"`)
- **THEN** the "Re-analyze Speakers" button SHALL show a loading state and be disabled

#### Scenario: Button hidden when models not downloaded
- **WHEN** the bundled enhanced diarization models are not available
- **THEN** the button SHALL not be visible; a "Setup Speaker Diarization" link SHALL point to settings