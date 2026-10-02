# Spec Delta

## MODIFIED Requirements

### Requirement: Device detection and reconnection
During a recording, the system SHALL detect that the active microphone (wired, USB or Bluetooth) has disconnected. It SHALL recover microphone capture by switching to the system default input device instead of waiting to reconnect the same device. The detection bound, the switch, its guards and its notifications are specified by the `mic-disconnect-recovery` capability.

#### Scenario: Detect AirPods disconnection
- **WHEN** the user removes AirPods (or powers off any headset) providing the recording microphone during recording
- **THEN** the system SHALL detect the disconnect within 10 seconds, switch microphone capture to the system default input, and show a notification naming the microphone now in use

#### Scenario: Disconnected device returns
- **WHEN** the original microphone reconnects after the system switched to the default input
- **THEN** the recording SHALL stay on the default input for the rest of the session
