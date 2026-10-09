## MODIFIED Requirements

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

## ADDED Requirements

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

## REMOVED Requirements

### Requirement: Per-channel live status lines
**Reason**: The lines were hidden whenever diarization was off, but the level bars and the speech-recognition buffer do not depend on diarization.
**Migration**: Replaced by "Live status lines for every recording", which keeps the same two lines and model row and shows them for every recording; with diarization off the diarization indicator is idle.
