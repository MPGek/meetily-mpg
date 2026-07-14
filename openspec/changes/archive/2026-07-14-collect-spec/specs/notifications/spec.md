## ADDED Requirements

### Requirement: Desktop notification delivery
The system SHALL deliver OS-level desktop notifications via Tauri's notification plugin for recording events, errors, and reminders.

#### Scenario: Show recording started notification
- **WHEN** user starts a meeting recording
- **THEN** system shows a high-priority desktop notification with title "Meetily" and body describing the action

### Requirement: Notification type categorization
The system SHALL classify notifications into types: RecordingStarted, RecordingStopped, RecordingPaused, RecordingResumed, TranscriptionComplete, MeetingReminder, SystemError, Test.

#### Scenario: Show transcription complete notification
- **WHEN** recording stops and audio is saved
- **THEN** system shows a Normal-priority notification with the file path of the saved recording

### Requirement: Notification priority levels
The system SHALL assign priorities to notifications: Low, Normal, High, Critical — influencing display behavior.

#### Scenario: System error gets critical priority
- **WHEN** an unrecoverable error occurs during transcription
- **THEN** system shows a notification with Critical priority that persists until dismissed

### Requirement: Notification consent management
The system SHALL track user consent for notifications and only deliver when consent is granted.

#### Scenario: User denies notification permission on first launch
- **WHEN** app requests OS notification permission and user clicks "Deny"
- **THEN** system sets consent=false and suppresses all subsequent desktop notifications

### Requirement: Do Not Disturb (DND) awareness
The system SHALL check the OS DND status before delivering notifications and skip when active.

#### Scenario: Show notification while OS DND is active
- **WHEN** user has Windows/macOS Focus Assist / Do Not Disturb enabled
- **THEN** system detects DND mode and suppresses desktop notifications (logged as suppressed)

### Requirement: Notification settings persistence
The system SHALL persist notification preferences (enabled/disabled, sound on/off) across app sessions.

#### Scenario: Toggle notification sound off
- **WHEN** user disables notification sounds in settings
- **THEN** all future notifications are delivered without playing a sound

### Requirement: Test notification support
The system SHALL provide a "Send Test Notification" command to verify the notification subsystem is working.

#### Scenario: Send test notification from settings
- **WHEN** user clicks "Test Notification" button in Settings > Notifications
- **THEN** system shows a simple informational desktop notification with message "This is a test notification"

### Requirement: Notification stats tracking
The system SHALL track and expose notification delivery statistics (total sent, suppressed by DND, errors).

#### Scenario: View notification delivery stats
- **WHEN** user opens Settings > Notifications and views stats panel
- **THEN** system displays counts of notifications sent, DND-suppressed, and failed deliveries
