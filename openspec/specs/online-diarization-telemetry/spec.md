# online-diarization-telemetry Specification

## Purpose

Gives users and developers live, per-channel visibility into online speaker diarization during a recording, so a channel that has stopped producing speaker embeddings, a low-confidence speaker match, or a broken turn timeline is visible while the meeting is still running instead of only in post-meeting logs.

## Requirements

### Requirement: Live status lines for every recording
The system SHALL display exactly two channel status lines while a recording is in progress, whatever the diarization mode, one for the microphone channel and one for the system channel, placed immediately to the right of the animated recording indicator, together with a single model-indicator row beneath them. No other status row SHALL be added.

#### Scenario: Two channel lines and one model row
- **WHEN** a recording is in progress with diarization mode set to Fast or Efficient
- **THEN** the recording controls display two channel lines to the right of the animated recording indicator, one for the microphone channel and one for the system channel, and beneath them a single row of model state indicators

#### Scenario: Shown when diarization is off
- **WHEN** a recording is in progress with diarization mode set to Off
- **THEN** the two channel lines and the model-indicator row are displayed, and the diarization indicator uses the idle treatment for a disabled model

#### Scenario: Hidden when not recording
- **WHEN** no recording is in progress
- **THEN** no channel lines and no model-indicator row are displayed

### Requirement: Channel status content
Each status line SHALL report only its own channel's state: the channel's audio level, the channel's speech-recognition buffer indicator, and, when the channel has blocks in flight, the count of blocks in queue. The line SHALL NOT report speech-chunk counts, embedding success/failure counts, buffered embedding counts, stable turn counts, or last-turn details.

#### Scenario: Queue depth for the recognition model
- **WHEN** a channel has accumulated merged blocks awaiting speech recognition
- **THEN** the status block shows the number of blocks pending before the recognition model

#### Scenario: Queue drained
- **WHEN** all blocks waiting for the recognition model have been processed
- **THEN** the pending count is no longer shown (zero/hidden), not a stale count

#### Scenario: Per-channel isolation
- **WHEN** the system channel receives no audio while the microphone channel receives speech
- **THEN** the system line's level, buffer indicator, and counts are unaffected by the microphone channel's activity

#### Scenario: Fast-mode accumulation
- **WHEN** Fast mode embeds speech chunks on the microphone channel
- **THEN** the microphone line shows that channel's level bar, its buffer indicator, and any pending-block count for its queues, without chunk or embedding counters

#### Scenario: No chunk or turn detail
- **WHEN** Fast mode embeds speech chunks on a channel or a stable turn with a match score is published
- **THEN** the line for that channel shows no chunk count, no embedding success/failure counts, no buffered-embedding count, no turn count, and no last-turn label, source, or score

#### Scenario: Latest turn with confidence
- **WHEN** a stable turn is published with a match score
- **THEN** no turn detail, attribution source, or score is rendered in the status block; only the diarization model's blink state and pending-block count update

### Requirement: Speech-recognition buffer indicator
Each channel line SHALL show the speech waiting to be sent for recognition as a tank against the 25 s accumulation cap, counting both speech already merged and speech still being collected, and SHALL show the silence after the pending speech as a countdown to the 500 ms dispatch trigger.

#### Scenario: Speech being collected
- **WHEN** a person is speaking on a channel and the voice activity detector has not yet closed the segment
- **THEN** that channel's tank grows with the length of that speech while it is spoken, not only when the segment closes

#### Scenario: Silence countdown
- **WHEN** a channel has pending speech and is silent
- **THEN** its countdown fills in proportion to the silence so far out of 500 ms, and its tank keeps showing the pending speech

#### Scenario: Dispatched
- **WHEN** a channel's pending speech is sent to recognition
- **THEN** its tank and countdown empty

#### Scenario: Approaching the cap
- **WHEN** a channel accumulates speech without a 500 ms pause
- **THEN** its tank shows the accumulated length against 25 s, and empties when the cap sends the speech to recognition

#### Scenario: Pending speech outlives the trigger
- **WHEN** a channel's pending speech is still unsent after its silence has exceeded the 500 ms trigger by more than one status refresh and one voice-activity window
- **THEN** that channel's tank uses a treatment visibly distinct from normal pending speech and the line states that it is waiting for speech

#### Scenario: Idle channel
- **WHEN** a channel has no pending speech and no speech in progress
- **THEN** its tank and countdown are empty without a warning or error treatment

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

### Requirement: Per-channel input level indicator
Each channel's line SHALL show a level bar for the audio actually being processed on that channel, so a silent or dead channel is distinguishable from a busy one at a glance. The bar SHALL reflect loudness on a decibel scale with a floor for practical silence, not a raw linear amplitude, and SHALL fall to empty within half a second when no audio arrives for that channel.

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
The model-indicator row SHALL show one indicator per behavioral model in use (voice activity, speech recognition, word alignment, and speaker diarization), each carrying the model's name so the row is readable without interaction. While that model has work in flight, its indicator SHALL blink: green when the model is currently processing a request, red when a request has been sent to the model but it is not yet processing. When loaded but with no work in flight, an indicator SHALL be a steady (non-blinking) healthy color; a model disabled by settings or not yet downloaded SHALL use the idle treatment, never the error treatment, and an indicator SHALL NOT be shown as healthy when the underlying model is not loaded. A model whose queue has dropped work SHALL use the warning treatment, and a model that has failed such that it can no longer process work SHALL use the error treatment.

#### Scenario: Processing
- **WHEN** a model is actively processing a request (recognition, alignment, or diarization work in progress)
- **THEN** its indicator blinks green

#### Scenario: Request sent, not yet processing
- **WHEN** a request has been submitted to a model but it has not started consuming it
- **THEN** its indicator blinks red

#### Scenario: Loaded and idle
- **WHEN** a model is loaded but has no work in flight
- **THEN** its indicator is a steady healthy color, not blinking

#### Scenario: Working model
- **WHEN** a model is loaded and consuming work
- **THEN** its indicator blinks green

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
The status block SHALL report, for each model the recording relies on (voice activity detection, speech recognition, word alignment, and speaker diarization), whether it is available and loaded, its identity under which it was loaded, and its current queue activity. For speech recognition, activity SHALL be the number of merged blocks pending before the model, cleared when the queue is drained. For diarization (and any other long-running model instrument), activity SHALL be the number of blocks pending before that model, decremented when a block is processed and cleared when all have processed. Voice activity SHALL be reported through its indicator state only; no frame counters SHALL be displayed.

#### Scenario: Recognition blocks pending
- **WHEN** merged speech blocks are queued for the recognition model and not yet recognized
- **THEN** the status block reports that pending count

#### Scenario: Diarization blocks pending
- **WHEN** blocks are enqueued to the diarization model and not yet processed
- **THEN** the status block reports the number of pending blocks, and the count is removed once those blocks have been processed

#### Scenario: Model not loaded
- **WHEN** a model in use has not been loaded
- **THEN** the status block reports it as not loaded rather than omitting it

#### Scenario: Model loaded and working
- **WHEN** a model is loaded and consuming work
- **THEN** the status block reports its identity and its pending-block activity

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
