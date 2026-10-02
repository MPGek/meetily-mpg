# summary-progress-tracking Specification

## Purpose

Keeps the meeting view's summary generation state consistent with the stored state of that meeting's summary run. This covers leaving and returning to a meeting, auto-summary, and stale or foreign results.

## Requirements

### Requirement: In-progress generation resumes on return
When a meeting is opened while its stored summary status is pending, the meeting view SHALL show generation in progress (regenerating if a previous summary exists) and SHALL keep tracking that run until it ends. The view SHALL then show the outcome as if the user had never left.

#### Scenario: Leave and return during generation
- **WHEN** the user starts a summary, opens another meeting, and returns while the run is still pending
- **THEN** the view shows generation in progress, and when the run completes it shows the new summary and the updated meeting title

#### Scenario: Return during regeneration keeps old notes visible
- **WHEN** the user returns to a meeting whose stored status is pending and which has a previously saved summary
- **THEN** the previous summary stays visible while the view shows regeneration in progress

#### Scenario: Stop after returning
- **WHEN** the user returns to a meeting with a pending run and clicks Stop
- **THEN** the cancel request targets that run and the view returns to idle

#### Scenario: Ordinary re-render
- **WHEN** the meeting view re-renders while a run is being tracked
- **THEN** tracking continues for the same run, with no second tracker started

### Requirement: Outcomes reached while away are shown on return
When a meeting is opened and its stored summary status is failed, the view SHALL show the stored error message without a new notification. When the stored status is completed or cancelled, the view SHALL show the stored summary without a new notification or completion analytics event.

#### Scenario: Run failed while the meeting was closed
- **WHEN** the user opens a meeting whose last run failed while another meeting was open
- **THEN** the view shows the stored error message, keeps the restored previous summary visible if one exists, and shows no toast

#### Scenario: Run completed while the meeting was closed
- **WHEN** the user opens a meeting whose run completed while another meeting was open
- **THEN** the view shows the stored summary with no success toast and no completion analytics event

### Requirement: Auto-summary starts only from a stored idle state
Automatic summary generation after a recording SHALL start only when the meeting's stored summary status is idle and the view is not already generating. It SHALL NOT start while a run is pending, or after a run has completed, failed, or been cancelled.

#### Scenario: Run already pending on arrival
- **WHEN** the user arrives from a recording with auto-summary enabled and the meeting's stored status is pending
- **THEN** no new run is started and the view tracks the pending run

#### Scenario: No prior run
- **WHEN** the user arrives from a recording with auto-summary enabled, the meeting has transcripts, and the stored status is idle
- **THEN** exactly one run is started

### Requirement: Results from another meeting or an older run are ignored
The meeting view SHALL apply a summary status result only if it belongs to the meeting currently shown and to the run being tracked. A stop request for a run SHALL NOT stop tracking of a different run of the same meeting.

#### Scenario: Late completion from the previous meeting
- **WHEN** a status request for meeting A completes after the user has switched to meeting B
- **THEN** meeting B's view does not change

#### Scenario: Status for another meeting at mount
- **WHEN** the view for meeting A receives a stored status that belongs to meeting B
- **THEN** meeting A's view stays idle and does not start tracking

#### Scenario: Result from an older run
- **WHEN** a status result's run id differs from the run being tracked
- **THEN** the result is ignored and tracking continues

#### Scenario: Old in-flight request after resume
- **WHEN** a status request from before the user left is still in flight when tracking resumes for the same run, and it then completes
- **THEN** it does not stop the resumed tracking, and the resumed tracking shows the run's outcome

#### Scenario: Leaving before the start response arrives
- **WHEN** the user leaves a meeting after requesting generation but before the start response arrives
- **THEN** the run is not cancelled, and returning to the meeting resumes tracking it

#### Scenario: Tracking one meeting does not stop another
- **WHEN** runs for two meetings are tracked at once and one of them ends
- **THEN** tracking of the other meeting's run continues
