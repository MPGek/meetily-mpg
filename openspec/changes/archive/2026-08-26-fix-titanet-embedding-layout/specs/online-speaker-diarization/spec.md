## ADDED Requirements

### Requirement: Online TitaNet embedding layout correctness

Online diarization (both Efficient and Fast modes) SHALL extract TitaNet-Large (192-d, `titanet_large`) embeddings using the same mel-as-dim-1 layout fix applied offline. Any per-chunk embedding buffered during recording and any stop-time embedding derived from those chunks SHALL be produced with the TitaNet-correct tensor layout, so recording-stop clustering does not replay the offline shape mismatch.

#### Scenario: Efficient mode buffers valid TitaNet embeddings

- **WHEN** a recording in Efficient mode captures several VAD speech chunks of varied length
- **THEN** each chunk's embedding SHALL be produced without `Got invalid dimensions for input: audio_signal Expected:80` errors, and the buffered set SHALL be 192-d vectors usable for `AhcClusterer`

#### Scenario: Fast mode stop-time grouping uses layout-correct embeddings

- **WHEN** a Fast-mode recording stops and stop-time grouping derives per-cluster embeddings from the buffered per-chunk embeddings (grouped by pipeline speaker identity and channel)
- **THEN** the derived embeddings SHALL be TitaNet-correct and SHALL not inflate clustering with layout-corrupted vectors

#### Scenario: Online batch and single-chunk paths agree

- **WHEN** online diarization embeds a batch of chunks versus one-by-one on the same audio
- **THEN** both paths SHALL produce order-preserving 192-d outputs with the same layout, matching the offline contract

### Requirement: Online diarization fails loudly when no valid embeddings buffered

If online embedding produces zero valid embeddings for a channel (every chunk failed), that channel's stop-time clustering SHALL skip that channel distinctly and the overall diarization SHALL NOT report a silent `0 speakers` success for that channel without a warning log. When both channels have zero valid embeddings and at least one chunk existed, the run SHALL allow offline diarization to be the recovery path and SHALL log the layout/underlying error context.

#### Scenario: No valid embeddings on a channel skips the channel

- **WHEN** Efficient or Fast mode buffered chunks for the microphone channel but every embedding for that channel failed
- **THEN** stop-time processing SHALL log a warning with the underlying error detail, SHALL NOT produce empty clusters for that channel, and SHALL still process the other channel's valid embeddings normally

## MODIFIED Requirements

### Requirement: Efficient mode extracts speaker embeddings during recording
The system SHALL extract speaker embeddings from VAD-detected speech segments during recording when Efficient mode is selected, buffering them in memory without performing clustering.

#### Scenario: Embedding extracted from speech segment
- **WHEN** the VAD detects a speech segment of at least 200ms duration during recording in Efficient mode
- **THEN** the system SHALL resample the segment to 16 kHz if needed and extract a speaker embedding vector using the enhanced TitaNet-Large embedder (192-d, layout-correct for `titanet_large`), storing it with the segment's start and end timestamps

#### Scenario: Silence periods skipped
- **WHEN** the VAD detects silence (no speech) for more than 500ms during recording in Efficient mode
- **THEN** the system SHALL NOT extract embeddings for the silence period

### Requirement: Fast mode extracts embeddings for recognition and enrollment
In Fast mode, the system SHALL run its own enhanced TitaNet-Large embedding extraction (192-d, layout-correct) on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does. The buffered embeddings SHALL be attributable to a specific pipeline speaker identity and channel so they can be grouped for enrollment without mixing channels.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks, each 192-d and produced with the TitaNet-correct layout

#### Scenario: Embeddings carry channel provenance
- **WHEN** buffered embeddings are grouped for enrollment at stop
- **THEN** each group SHALL be identifiable by channel and pipeline speaker identity, and groupings SHALL never span both channels
