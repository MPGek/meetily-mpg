## MODIFIED Requirements

### Requirement: Summary process tracking
The system SHALL track summary generation jobs per meeting with status, result JSON, and timing metadata. Each run SHALL be identified by its recorded start time. A completed, failed, or cancelled update SHALL apply only while the row is still pending with that run's start time, and SHALL report whether it applied. Rows left pending by a previous app process SHALL be marked failed at startup.

#### Scenario: Record summary completion
- **WHEN** a summary finishes generating via LLM provider
- **THEN** system updates the `summary_processes` table with completed status, result JSON, chunk count, and processing duration

#### Scenario: Terminal update for a stale run is rejected
- **WHEN** a completed, failed, or cancelled update arrives for a meeting with a start time that differs from the row's current start time
- **THEN** the row is unchanged and the update reports that it did not apply

#### Scenario: First terminal update wins
- **WHEN** a run's row has already been marked cancelled and a completed update for the same run arrives
- **THEN** the row stays cancelled with the restored previous result, and the completed update reports that it did not apply

#### Scenario: Run identity survives the round trip
- **WHEN** a run is started and the meeting's summary status is read back
- **THEN** the status response's start value equals the run id returned when the run was started

#### Scenario: Interrupted run is failed at startup
- **WHEN** the app starts and a `summary_processes` row is still pending from a previous process
- **THEN** the row is marked failed with an error saying generation was interrupted, and the backed-up previous result, if any, is restored as the row's result
