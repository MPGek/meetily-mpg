# mic-disconnect-recovery Specification

## Purpose

Keeps a meeting recording capturing the user's voice when the microphone in use becomes unavailable. At recording start, an unavailable microphone resolves to a present one. During a recording, a disconnected microphone is replaced by the system default input. The user is told what happened, and the transcript, live speaker labels and saved audio stay continuous and time-aligned across the switch.

## Requirements

### Requirement: Mid-recording microphone disconnect falls back to the default input
When the microphone in use disconnects during an active recording, the system SHALL switch microphone capture to the current system default input device without stopping the recording. The backend SHALL do this by itself; the frontend SHALL NOT need to poll, and the current page SHALL NOT matter. System-audio capture SHALL continue uninterrupted during the switch.

#### Scenario: USB microphone unplugged mid-recording
- **WHEN** the user unplugs the USB microphone in use while a recording is active and another input device is available
- **THEN** within 10 seconds the recording SHALL resume capturing microphone audio from the system default input device, and the recording SHALL remain active

#### Scenario: Bluetooth headset powered off mid-recording
- **WHEN** the Bluetooth headset providing the recording microphone is powered off during a recording
- **THEN** the system SHALL switch microphone capture to the system default input device that remains, without user action

#### Scenario: Recovery does not depend on the frontend
- **WHEN** the microphone disconnects while the app window shows a page other than the recording page, or while the frontend is not polling anything
- **THEN** the switch SHALL still happen

#### Scenario: System audio keeps recording during the switch
- **WHEN** the microphone switch is in progress
- **THEN** system-audio capture SHALL NOT be stopped or restarted

### Requirement: Only the active microphone triggers a switch
The system SHALL switch the microphone only when the device that disconnected is the microphone this session is currently capturing from. Disconnect events for any other device SHALL NOT change the microphone. This includes the system-audio device and a microphone the session already switched away from.

#### Scenario: System-audio device disconnects
- **WHEN** the system-audio device disconnects during a recording while the microphone stays connected
- **THEN** the microphone SHALL NOT be switched, and no microphone-switch notification SHALL be shown

#### Scenario: Late event for the previous microphone
- **WHEN** a disconnect event for the original microphone arrives after the session has already switched to the default input
- **THEN** the system SHALL ignore it and keep capturing from the current microphone

### Requirement: The fallback microphone is kept for the rest of the session
After a successful switch, the system SHALL keep recording from the fallback microphone until the recording stops, even if the original device reconnects. If the fallback microphone itself later disconnects, the system SHALL treat that as a new disconnect of the active microphone.

#### Scenario: Original device reconnects
- **WHEN** the original microphone reconnects after the session switched to the default input
- **THEN** the recording SHALL continue on the fallback microphone, and no further switch SHALL occur

#### Scenario: Fallback microphone disconnects too
- **WHEN** the fallback microphone disconnects later in the same recording
- **THEN** the system SHALL attempt another switch to the then-current default input

### Requirement: A switch never outlives its recording session
A microphone switch SHALL take effect only on the recording session that started it. If that session stops, or is replaced by a new recording, before the switch completes, the system SHALL discard the switch and release the replacement device. A discarded switch SHALL NOT change the new session's devices and SHALL NOT notify the user.

#### Scenario: Stop pressed while a switch is in progress
- **WHEN** the user stops the recording while a microphone switch is in progress
- **THEN** the stop SHALL complete normally, the replacement microphone SHALL be released, and no switch failure SHALL be reported

#### Scenario: Stop then immediate Start
- **WHEN** the user stops a recording whose microphone just disconnected and immediately starts a new recording
- **THEN** the new recording SHALL capture from the devices resolved at its own start, and SHALL NOT be switched or notified because of the previous session's disconnect

### Requirement: Microphone recovery attempts are bounded per session
The system SHALL retry a failed switch while the microphone in use is still missing, up to 3 failed attempts per disconnect. After the third failure it SHALL stop retrying for that disconnect and keep the recording running without a microphone. A successful switch, or a new recording session, SHALL start a fresh budget. An attempt abandoned because its session ended SHALL NOT count.

#### Scenario: No other input device is available
- **WHEN** the only microphone disconnects and no default input device is available
- **THEN** the system SHALL make at most 3 switch attempts, then stop trying, and the recording SHALL continue capturing system audio

#### Scenario: Default input still reports the lost device
- **WHEN** the system default input still names the disconnected device after a short settle wait and one re-check
- **THEN** that attempt SHALL count as failed and SHALL NOT reopen the lost device

#### Scenario: New session gets a fresh budget
- **WHEN** a previous recording exhausted its recovery attempts and the user starts a new recording
- **THEN** the new recording SHALL get the full number of recovery attempts if its microphone disconnects

### Requirement: The user is told about microphone switches and recovery failures
The system SHALL notify the user in the app when the microphone in use is replaced. The notification SHALL name the new device. The system SHALL also notify when an attempt fails and when it gives up, saying that the recording continues without a microphone. Notifications SHALL appear on any page and SHALL NOT be shown for a session that has stopped. Each new notification SHALL replace the previous one instead of stacking.

#### Scenario: Successful switch
- **WHEN** the microphone switch succeeds
- **THEN** the user SHALL see a notification naming the microphone now in use for the meeting

#### Scenario: Failed attempt with retries remaining
- **WHEN** a switch attempt fails and attempts remain
- **THEN** the user SHALL see a warning that the microphone was lost and that recovery is being retried

#### Scenario: Recovery exhausted
- **WHEN** the last allowed switch attempt fails
- **THEN** the user SHALL see an error saying the microphone could not be recovered, that the recording continues without it, and that stopping and restarting the recording may help

#### Scenario: No notification after Stop
- **WHEN** a switch attempt fails or is discarded because the recording was stopped
- **THEN** the user SHALL NOT see a switch failure or recovery-exhausted notification

### Requirement: Recording starts on a microphone that is actually present
At recording start, the system SHALL use a requested or preferred microphone only if a present input device matches it. Detection SHALL then watch that exact device. If none is present, recording SHALL start on the system default input and the user SHALL be told which microphone is in use. If no input is available at all, start SHALL fail with an error, as it does today.

#### Scenario: Preferred microphone not plugged in at start
- **WHEN** recording starts while the saved preferred microphone is not connected and a default input exists
- **THEN** recording SHALL start on the default input, the user SHALL see a notification naming it, and no mid-recording microphone switch SHALL be triggered by the missing preferred device

#### Scenario: Preferred microphone present
- **WHEN** recording starts while the preferred microphone is connected
- **THEN** recording SHALL start on it and no switch notification SHALL be shown

### Requirement: Speech in progress on the lost microphone is closed at the switch
When the microphone is switched, the system SHALL end any speech segment that was in progress on the lost microphone using only the audio captured before the disconnect. It SHALL transcribe that segment, and SHALL NOT merge it with speech captured by the replacement microphone.

#### Scenario: User was speaking when the microphone died
- **WHEN** the microphone disconnects in the middle of the user's utterance and the system later switches to the default input
- **THEN** the words captured before the disconnect SHALL appear as their own transcript segment, and speech after the switch SHALL start a new segment

### Requirement: Transcript timing stays aligned across the switch
Transcript segments from the replacement microphone SHALL carry recording-relative times that match when the speech actually happened. They SHALL NOT shift earlier by the duration of the disconnect gap.

#### Scenario: Speech right after the switch
- **WHEN** the user speaks 2 seconds after the switch completes, 8 seconds after the disconnect
- **THEN** the transcript segment's start time SHALL be within 1 second of the real elapsed recording time of that speech

### Requirement: The saved microphone track stays time-aligned across the switch
The saved recording SHALL keep the microphone channel aligned in time with the system-audio channel and with transcript times across the switch. The disconnect gap SHALL be recorded as silence on the microphone channel, with no audio dropped or shifted. Silence inserted for one gap SHALL NOT exceed 30 seconds.

#### Scenario: System audio active during the gap
- **WHEN** system audio plays throughout the microphone gap
- **THEN** the saved left (microphone) channel SHALL be silent for the gap, and microphone audio after the switch SHALL line up with the system audio recorded at the same moment

#### Scenario: No system audio during the gap
- **WHEN** no system audio is captured during the gap, because the system is silent or there is no system-audio device
- **THEN** the saved file SHALL still contain the gap as silence, so microphone audio after the switch plays at the same recording time as its transcript segment

#### Scenario: Recovery never succeeds
- **WHEN** no switch succeeds before the recording stops
- **THEN** no silence SHALL be inserted for the unrecovered gap, and the existing saved-audio duration check SHALL report any resulting shortfall

### Requirement: A replacement microphone with a different format records correctly
The system SHALL accept a replacement microphone whose native sample rate or channel count differs from the original's. Examples are a 16 kHz Bluetooth hands-free microphone and a 44.1 kHz or 48 kHz device. Its audio SHALL be converted the same way as a microphone chosen at start. Saved audio and transcription after the switch SHALL play at normal speed and pitch.

#### Scenario: Switch from a 48 kHz microphone to a 16 kHz headset microphone
- **WHEN** the microphone switch lands on a device reporting 16 kHz mono
- **THEN** the saved microphone audio after the switch SHALL have correct speed and pitch, and its speech SHALL be transcribed

#### Scenario: Switch from a 16 kHz headset microphone to a 48 kHz built-in array
- **WHEN** a 16 kHz Bluetooth microphone disconnects and the switch lands on a 48 kHz device
- **THEN** recording and transcription SHALL continue correctly on the new device

### Requirement: A replacement microphone gets the same processing as a start-time microphone
The replacement microphone SHALL go through the same processing chain as a microphone chosen at recording start: high-pass filter and, when enabled, noise suppression. It SHALL stay at natural (unity) gain with no automatic gain. No per-device gain setting SHALL need to be carried over.

#### Scenario: Quiet room after the switch
- **WHEN** the replacement microphone captures only ambient noise
- **THEN** its signal SHALL NOT be amplified toward a loudness target

### Requirement: Live speaker labeling continues on the microphone channel across the switch
During a recording with online diarization, the microphone channel's live speaker state SHALL continue across a switch and SHALL NOT be reset. This state includes clusters, recognized speakers and the user's live assignments. Speech from the replacement microphone SHALL be labeled on the microphone channel. Stop-time speaker finalization SHALL cover speech from before and after the switch.

#### Scenario: Live rename survives a switch
- **WHEN** the user renamed a live microphone-channel speaker before the microphone disconnected, and that speaker keeps talking after the switch
- **THEN** existing labels and the user's assignment SHALL remain, and post-switch speech SHALL be processed on the microphone channel

#### Scenario: Stop after a switch
- **WHEN** a recording with online diarization stops after a microphone switch
- **THEN** stop-time speaker labels SHALL be written for microphone transcripts from both before and after the switch

### Requirement: Microphone switch decisions are logged
The system SHALL log each step and outcome of a microphone recovery with a stable `[HOT_SWAP]` tag, so a support log shows when and how the microphone changed. Logged items: disconnect detected, target device, switch completed (with the new device's sample rate), attempt failed (with attempt number), switch discarded (with reason), and recovery exhausted.

#### Scenario: Successful switch in the log
- **WHEN** a microphone switch completes
- **THEN** the application log SHALL contain `[HOT_SWAP]` lines naming the lost device, the new device and its sample rate
