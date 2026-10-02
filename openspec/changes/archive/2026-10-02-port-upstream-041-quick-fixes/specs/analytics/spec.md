# Spec Delta

## MODIFIED Requirements

### Requirement: Session management
The system SHALL track user sessions with start time, duration, and active state for analytics aggregation. Ending a session SHALL complete promptly and SHALL NOT block later analytics calls.

#### Scenario: Start new analytics session on app launch
- **WHEN** the app is launched (or resumed from background)
- **THEN** system creates a new UserSession with a UUID-based session_id and marks it as active

#### Scenario: End the active session
- **WHEN** the active session is ended (for example when the webview unloads)
- **THEN** the system SHALL clear the active session, send one "session_ended" event carrying that session's id and duration, and return without waiting on its own session state

#### Scenario: Events after a session ends are not blocked
- **WHEN** an analytics event is tracked after the active session has been ended
- **THEN** the event call SHALL complete (sent, or skipped per consent) instead of hanging
