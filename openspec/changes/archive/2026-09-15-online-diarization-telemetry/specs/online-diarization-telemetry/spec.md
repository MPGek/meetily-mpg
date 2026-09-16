## Purpose

Gives users and developers live, per-channel visibility into online speaker diarization during a recording, so a channel that has stopped producing speaker embeddings, a low-confidence speaker match, or a broken turn timeline is visible while the meeting is still running instead of only in post-meeting logs.

## ADDED Requirements

### Requirement: Per-channel live status lines
The system SHALL display exactly two channel status lines while a recording with online diarization active is in progress, one for the microphone channel and one for the system channel, placed immediately to the right of the animated recording indicator, together with a single model-indicator row beneath them. No other status row SHALL be added.

#### Scenario: Two channel lines and one model row
- **WHEN** a recording is in progress with diarization mode set to Fast or Efficient
- **THEN** the recording controls display two channel lines to the right of the animated recording indicator, one for the microphone channel and one for the system channel, and beneath them a single row of model state indicators

#### Scenario: Hidden when diarization is off
- **WHEN** a recording is in progress with diarization mode set to Off
- **THEN** no channel lines and no model-indicator row are displayed

#### Scenario: Hidden when not recording
- **WHEN** no recording is in progress
- **THEN** no channel lines and no model-indicator row are displayed

### Requirement: Channel status content
Each status line SHALL report only its own channel's state: the number of speech chunks received, the count of successful and failed embedding attempts, the number of buffered embeddings, the number of stable speaker turns, and the most recent turn with its speaker label or display name, its attribution source, and its match score.

#### Scenario: Fast-mode accumulation
- **WHEN** Fast mode embeds speech chunks on the microphone channel
- **THEN** the microphone line reports that channel's chunk count, embedding success count, buffered embedding count, and stable turn count

#### Scenario: Per-channel isolation
- **WHEN** the system channel receives no audio while the microphone channel receives speech
- **THEN** the system line reports no activity for the system channel and the microphone line's counts are unaffected by the system channel

#### Scenario: Latest turn with confidence
- **WHEN** a stable turn is published with a match score
- **THEN** that turn's speaker label or display name, attribution source, and score are shown on the line for that turn's channel

### Requirement: Empty and degraded channel reporting
The system SHALL distinguish "no activity yet", "not applicable", and "expected zero" from an actual failure: a recording without a system audio device SHALL be reported as a mono session rather than as an inactive channel, and a channel in Efficient mode SHALL state that clustering is deferred to recording stop rather than presenting a zero turn count as an anomaly.

#### Scenario: Mono session
- **WHEN** the recording has no system audio device
- **THEN** the system line reports a mono session and does not present that channel's counters as a fault

#### Scenario: Efficient mode
- **WHEN** the diarization mode is Efficient
- **THEN** each line states that clustering is deferred to recording stop and the zero stable-turn count is not rendered as a warning

#### Scenario: No speech yet
- **WHEN** a channel has received no speech chunks
- **THEN** its line reports zero activity without an error or warning indication

### Requirement: Channel health signals
The system SHALL surface a channel's failure as soon as that channel stops producing embeddings for the session, and SHALL flag a channel whose published turns are no longer ordered in time, using a visually distinct error treatment for the former and warning treatment for the latter.

#### Scenario: Embedding stops
- **WHEN** a channel fails to embed a chunk such that the session's diarization is disabled
- **THEN** that channel's line is rendered as an error state

#### Scenario: Turn order regression
- **WHEN** a channel's latest published turn starts or ends before the previous turn on that same channel
- **THEN** that channel's line is rendered with a warning

#### Scenario: Healthy session
- **WHEN** a channel continues to embed successfully and publishes turns in time order
- **THEN** its line is rendered in the normal state with neither warning nor error

### Requirement: Global context discoverability
The system SHALL make the context required to interpret a reported match score discoverable from the status block — the active diarization mode, the active speaker model's identifier and embedding dimension, the recognition threshold, and the counts of loaded prototypes and session bindings — without adding a third status line.

#### Scenario: Threshold alongside score
- **WHEN** a line reports a match score
- **THEN** the recognition threshold used to accept that score is discoverable from the status block

#### Scenario: Mode and model
- **WHEN** diarization is active
- **THEN** the active mode and the active speaker model's identifier are discoverable from the status block

#### Scenario: Unavailable prototype context
- **WHEN** no prototype store is loaded for the session
- **THEN** the status block reports that prototype context is unavailable instead of presenting zero counts as real values

### Requirement: Non-interference with the recording pipeline
Status sampling SHALL occur on a fixed bounded interval and SHALL NOT emit an event per audio chunk, and displaying the status lines SHALL NOT alter transcription, voice activity detection, recording, or diarization output.

#### Scenario: Bounded sampling
- **WHEN** a recording with diarization active is in progress
- **THEN** the status is refreshed on a fixed interval and no status update is emitted for each audio chunk

#### Scenario: Unchanged outputs
- **WHEN** the status lines are displayed for an entire recording
- **THEN** the produced transcript, the speaker assignments, and the saved audio are identical to the same recording made without the status lines

### Requirement: Session scoping
The status SHALL reflect only the current recording session: it SHALL be reset when a new recording starts and SHALL NOT present values left over from a previous session as live values.

#### Scenario: Fresh session
- **WHEN** a new recording starts after a previous online diarization session
- **THEN** the lines begin from zero activity instead of repeating the previous session's counts

#### Scenario: After recording stop
- **WHEN** a recording has stopped and no diarization session is active
- **THEN** counters from the stopped session are not presented as live values

### Requirement: Buffer fill and next-fire indication
For every buffer that must fill before an operation fires, each channel's line SHALL show the buffer's current fill as a proportional bar whose length is the fill relative to its trigger threshold, together with a short label naming the operation it gates, so it is visible how close that channel is to triggering: the voice-activity dispatch window, the pending speech accumulation that is merged and sent for recognition, and the recording mix window that produces a saved stereo block. A buffer below its threshold SHALL read as filling, never as an error. A buffer whose fill exceeds its threshold SHALL render as a full bar rather than overflowing its track, and the firing SHALL be indicated separately.

#### Scenario: Filling a gated buffer
- **WHEN** a channel has accumulated part of a buffer's trigger threshold
- **THEN** that channel's line shows a bar filled to that proportion and the name of the operation the buffer gates

#### Scenario: Threshold reached
- **WHEN** a channel's buffer has reached or exceeded its trigger threshold
- **THEN** the bar renders full and the line indicates that the gated operation has fired

#### Scenario: Idle buffer is not a fault
- **WHEN** a buffer is empty or barely filled because the channel has no speech
- **THEN** the bar renders empty or barely filled without an error or warning treatment

### Requirement: Per-channel input level indicator
Each channel's line SHALL show a level bar for the audio actually being processed on that channel, so a silent or dead channel is distinguishable from a busy one at a glance. The bar SHALL reflect loudness on a decibel scale with a floor for practical silence, not a raw linear amplitude, and SHALL fall to empty within half a second when no audio arrives for that channel, so a paused or disconnected channel never keeps showing its last value.

#### Scenario: Speech on a channel
- **WHEN** a channel is receiving speech at a normal level
- **THEN** its level bar is visibly filled

#### Scenario: Quiet or silent channel
- **WHEN** a channel receives silence or only room noise
- **THEN** its level bar is empty or near empty without an error or warning treatment

#### Scenario: Audio stops arriving
- **WHEN** no audio has arrived on a channel for half a second
- **THEN** that channel's level bar has fallen to empty instead of holding its last value

#### Scenario: Channels are independent
- **WHEN** one channel is loud while the other is silent
- **THEN** only the loud channel's level bar is filled

### Requirement: Visual model state indicators
The model-indicator row SHALL show one indicator per model kind in use — voice activity, speech recognition, word alignment, and speaker diarization — using a distinct colour per state, and each indicator SHALL carry the model's name so the row is readable without interaction. The colours SHALL mean: healthy (loaded and working), idle or disabled (not applicable to this session), warning (working but degraded), and error (failed). A model that is disabled by settings or not yet downloaded SHALL use the idle treatment, never the error treatment, and an indicator SHALL NOT be shown as healthy when the underlying model is not loaded.

#### Scenario: Working model
- **WHEN** a model is loaded and consuming work
- **THEN** its indicator uses the healthy colour

#### Scenario: Disabled or idle model
- **WHEN** a model is disabled by settings, not downloaded, or unused by the session
- **THEN** its indicator uses the idle colour and not the error colour

#### Scenario: Degraded model
- **WHEN** a model is working but its queue has dropped work
- **THEN** its indicator uses the warning colour

#### Scenario: Failed model
- **WHEN** a model has failed such that it can no longer process work
- **THEN** its indicator uses the error colour

### Requirement: Activity of every model kind in use
The status block SHALL report, for each model the recording relies on — voice activity detection, speech recognition, word alignment, and speaker diarization — whether it is available and loaded, its identity under which it was loaded, and its current activity. Activity SHALL mean consumed work rather than only readiness: for voice activity, speech frames evaluated and whether speech is currently detected; for speech recognition, the queue depth of pending segments and the most recent recognition; for word alignment, whether the engine is loaded, queue occupancy, and refinements produced; for diarization, the per-channel counters already required above.

#### Scenario: Model not loaded
- **WHEN** a model in use has not been loaded
- **THEN** the status block reports it as not loaded rather than omitting it

#### Scenario: Model loaded and working
- **WHEN** a model is loaded and consuming work
- **THEN** the status block reports its identity and its activity counters

#### Scenario: Model disabled or unused
- **WHEN** a model is disabled by settings or not required by the current session
- **THEN** the status block reports it as disabled or not in use rather than as a failure

### Requirement: Status refresh rate
The status SHALL be refreshed at least every 150 milliseconds while a recording is in progress, using the recording controls' existing sampling interval rather than adding a second timer, and SHALL still emit nothing per audio chunk.

#### Scenario: Refreshed at the required rate
- **WHEN** a recording with diarization active is in progress
- **THEN** the status is refreshed on the recording controls' interval, which is at most 150 milliseconds

#### Scenario: No extra timer
- **WHEN** the status lines are displayed
- **THEN** no additional sampling timer is registered beyond the recording controls' existing interval
