## MODIFIED Requirements

### Requirement: Channel status content
Each status line SHALL report only its own channel's state: the channel's audio level and, when the channel has blocks in flight, the count of blocks in queue. The line SHALL NOT report speech-chunk counts, embedding success/failure counts, buffered embedding counts, stable turn counts, or last-turn details.

#### Scenario: Queue depth for the recognition model
- **WHEN** a channel has accumulated merged blocks awaiting speech recognition
- **THEN** the status block shows the number of blocks pending before the recognition model

#### Scenario: Queue drained
- **WHEN** all blocks waiting for the recognition model have been processed
- **THEN** the pending count is no longer shown (zero/hidden), not a stale count

#### Scenario: Per-channel isolation
- **WHEN** the system channel receives no audio while the microphone channel receives speech
- **THEN** the system line's level and counts are unaffected by the microphone channel's activity

#### Scenario: Fast-mode accumulation
- **WHEN** Fast mode embeds speech chunks on the microphone channel
- **THEN** the microphone line shows that channel's level bar and any pending-block count for its queues, without chunk or embedding counters

#### Scenario: No chunk or turn detail
- **WHEN** Fast mode embeds speech chunks on a channel or a stable turn with a match score is published
- **THEN** the line for that channel shows no chunk count, no embedding success/failure counts, no buffered-embedding count, no turn count, and no last-turn label, source, or score

#### Scenario: Latest turn with confidence
- **WHEN** a stable turn is published with a match score
- **THEN** no turn detail, attribution source, or score is rendered in the status block; only the diarization model's blink state and pending-block count update

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

## REMOVED Requirements

### Requirement: Global context discoverability
**Reason**: The user removed the detailed per-model visualization. The threshold, model identifier/dimension, mode, and prototype context are no longer displayed in the status block; the simplified block shows only levels, queue counters, and blink indicators.
**Migration**: The backend commands and counters remain available for debugging via logs and the telemetry snapshot; no replacement UI is provided.

### Requirement: Buffer fill and next-fire indication
**Reason**: Buffer-fill bars (voice-activity dispatch window, pending speech accumulation, recording mix window) are part of the per-action visualization the user asked to remove. Buffer state is now conveyed only through the model blink indicators and queue counters.
**Migration**: none; buffer state remains visible indirectly through indicator states.
