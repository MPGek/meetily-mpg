# Spec Delta

## ADDED Requirements

### Requirement: Saved recording stays on the session timeline
The saved stereo file SHALL keep both channels on the session's elapsed-time timeline for the whole recording. Audio captured at elapsed time t SHALL play at time t of the saved file, within 0.5 seconds, however long the meeting is and however unevenly the two channels' audio arrives. Silence SHALL be added to a channel only while that channel delivers no audio.

#### Scenario: Long meeting with both channels delivering
- **WHEN** a 60-minute meeting is recorded with the microphone and system audio both delivering audio throughout
- **THEN** speech captured at minute 59 SHALL play within 0.5 seconds of minute 59 in the saved file, and the saved file's duration SHALL be within 0.5 seconds of the session's recorded duration

#### Scenario: Channels deliver audio unevenly
- **WHEN** the two channels deliver audio in batches of different sizes, with timing jitter of up to 100 ms between them
- **THEN** no silence SHALL be inserted into either channel and the saved file SHALL NOT become longer than the session

#### Scenario: Playing a transcript block late in the meeting
- **WHEN** the user plays a transcript block that starts 24 minutes into a recording
- **THEN** playback SHALL start at that block's own speech, not at speech captured earlier in the meeting

### Requirement: A channel that stops delivering is recorded as silence
When one channel delivers no audio while the other keeps delivering, for example system audio while the output is silent or the microphone during a device switch, the system SHALL keep recording the live channel without gaps and SHALL record silence on the silent channel for that span.

#### Scenario: System output is silent for a minute
- **WHEN** system audio delivers nothing for 60 seconds while the microphone keeps delivering
- **THEN** the left (microphone) channel SHALL be continuous across those 60 seconds, the right (system) channel SHALL be silent for them, and the saved file's duration SHALL still match the session

### Requirement: A channel that resumes lines up with the other channel
When a channel that delivered no audio starts delivering again, the system SHALL place its audio at the moment it resumed, aligned within 50 milliseconds with the other channel's audio captured at the same moment.

#### Scenario: System audio resumes after a silent span
- **WHEN** system audio resumes after delivering nothing for some seconds, and a sound reaches both channels at the same moment after the resume
- **THEN** that sound SHALL sit at the same position in both channels of the saved file, within 50 milliseconds

### Requirement: The end of a recording is saved
When a recording stops, the system SHALL save all mixed audio captured before the stop, including audio that has not yet filled a complete mixing window, instead of dropping the final fraction of a second.

#### Scenario: Word spoken just before stopping
- **WHEN** the user says a short word in the last half second before stopping the recording
- **THEN** that word SHALL be present in the saved file
