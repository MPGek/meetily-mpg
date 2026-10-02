# analytics Specification

## Purpose
TBD - created by archiving change collect-spec. Update Purpose after archive.
## Requirements
### Requirement: PostHog analytics integration
The system SHALL integrate with PostHog for event tracking using the `posthog_rs` SDK for anonymous usage analytics.

#### Scenario: Initialize analytics client
- **WHEN** app starts and analytics is enabled in config
- **THEN** system creates a PostHog HTTP client configured with the project API key and host URL

### Requirement: Event tracking for user actions
The system SHALL track named events (meeting_started, recording_started, recording_stopped, summary_generated, etc.) with structured properties.

#### Scenario: Track meeting started event
- **WHEN** user begins a new meeting recording session
- **THEN** system sends an analytics event "meeting_started" with properties {meeting_id, timestamp} to PostHog

### Requirement: User identification
The system SHALL associate analytics events with a persistent anonymous user ID for session tracking.

#### Scenario: Identify user across sessions
- **WHEN** app starts for the first time in a session
- **THEN** system generates or loads a persisted UUID and identifies it via PostHog's identify call

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

### Requirement: Analytics consent gating
The system SHALL only send events when user has granted analytics consent, defaulting to disabled.

#### Scenario: Send event with analytics disabled
- **WHEN** config.enabled is false and user starts recording
- **THEN** system skips the PostHog event send (no error, silent no-op)

### Requirement: Sensitive property sanitization
The system SHALL strip PII fields from analytics events before sending to PostHog.

#### Scenario: Sanitize meeting properties before tracking
- **WHEN** an analytics event includes keys like "meeting_title", "file_path", or "device_name"
- **THEN** system removes those keys (listed in SENSITIVE_ANALYTICS_KEYS) before sending the event

### Requirement: Analytics-enabled toggle command
The system SHALL provide a Tauri command to enable/disable analytics at runtime, persisting the preference.

#### Scenario: Enable analytics from settings
- **WHEN** user toggles "Send anonymous usage data" in Settings
- **THEN** system updates config.enabled and initializes or shuts down the PostHog client accordingly
