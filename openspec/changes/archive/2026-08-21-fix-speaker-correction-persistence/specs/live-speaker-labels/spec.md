## ADDED Requirements

### Requirement: Live user bindings persist deterministically on stop
When a live recording stops, the system SHALL persist user-assigned speaker identities into the stored data such that reopening the meeting deterministically shows the user's name for corrected blocks and clusters, independent of the render-time join alone. User-bound clusters SHALL be written to `meeting_speakers` with `matched_by='user'`, and single-turn overrides SHALL be written to the matching transcript's override, before the recording is considered finalized. Finalization SHALL run for every stopped recording that used live diarization, and a user binding SHALL NOT be silently dropped when the post-stop restore step cannot run.

#### Scenario: Corrected cluster shows the user's name after restart
- **WHEN** a user renames live cluster `SPEAKER_01` to "Alice" during a Fast-mode recording and then stops it
- **THEN** reopening the meeting SHALL show "Alice" for that cluster's transcripts with user provenance, without requiring any further user action after stop

#### Scenario: Single-turn override survives stop and reopen
- **WHEN** the user relabels one live turn to "Bob" and stops the recording
- **THEN** the transcript matched to that turn SHALL show "Bob" with user provenance after reopening

#### Scenario: Finalize always runs for live-diarized recordings
- **WHEN** a recording used live (Fast-mode) diarization and stops
- **THEN** the finalization SHALL run automatically and SHALL not be skipped based on a frontend flag that can be false when live bindings were made

#### Scenario: Assignment failure is not silent
- **WHEN** a user attempts a live speaker assignment but no live prototype store is active for the session
- **THEN** the assignment SHALL fail loudly (reported to the user) rather than succeed silently and later revert to a predicted label
