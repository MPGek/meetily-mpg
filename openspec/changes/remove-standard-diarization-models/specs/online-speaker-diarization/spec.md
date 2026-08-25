# Delta spec: online-speaker-diarization

## MODIFIED Requirements

### Requirement: Efficient mode extracts speaker embeddings during recording
The system SHALL extract speaker embeddings from VAD-detected speech segments during recording when Efficient mode is selected, buffering them in memory without performing clustering.

#### Scenario: Embedding extracted from speech segment
- **WHEN** the VAD detects a speech segment of at least 200ms duration during recording in Efficient mode
- **THEN** the system SHALL resample the segment to 16 kHz if needed and extract a speaker embedding vector using the enhanced TitaNet-Large embedder (the model-aware embedder backed by the bundled enhanced model set), storing it with the segment's start and end timestamps

#### Scenario: Silence periods skipped
- **WHEN** the VAD detects silence (no speech) for more than 500ms during recording in Efficient mode
- **THEN** the system SHALL NOT extract embeddings for the silence period

### Requirement: Graceful fallback to offline diarization
The system SHALL fall back to the existing offline diarization pipeline if online processing fails for any reason.

#### Scenario: Streaming diarization initialization failure
- **WHEN** the polyvoice `StreamingPipeline` fails to initialize (e.g., bundled enhanced model files missing or corrupt) in Fast mode
- **THEN** the system SHALL log the error, emit a warning event to the frontend, and continue recording without diarization; offline diarization remains available after recording stops (but also fails if the enhanced models are missing)

#### Scenario: Embedding extraction runtime error
- **WHEN** the enhanced embedding model fails during recording in Efficient mode
- **THEN** the system SHALL log the error, clear the embedding buffer, and allow offline diarization to run after recording stops

### Requirement: Efficient mode clusters with the calibrated threshold

The system SHALL cluster the buffered speaker embeddings at recording stop in Efficient mode using the same fixed cosine-similarity threshold as the enhanced TitaNet-Large offline family, so distinct speakers are assigned distinct labels and online/offline results stay consistent.

#### Scenario: Multi-speaker recording produces distinct labels

- **WHEN** recording stops in Efficient mode with buffered embeddings containing multiple distinct speakers on a single channel
- **THEN** the clustering SHALL produce more than one cluster and matched transcripts SHALL be assigned distinct speaker IDs

#### Scenario: Clustering matches offline label scheme

- **WHEN** an online-Efficient-diarized meeting is re-analyzed with offline diarization
- **THEN** both paths SHALL apply the same enhanced family threshold, producing the same `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and comparable speaker counts

### Requirement: Fast mode extracts embeddings for recognition and enrollment
In Fast mode, the system SHALL run its own TitaNet-Large embedding extraction on each VAD-filtered speech chunk per channel (in addition to the polyvoice StreamingPipeline, whose turns carry no embeddings) and buffer the embeddings with timestamps for the duration of the session. Efficient mode SHALL continue to buffer per-chunk embeddings as it already does. The buffered embeddings SHALL be attributable to a specific pipeline speaker identity and channel so they can be grouped for enrollment without mixing channels.

#### Scenario: Embeddings available at stop in both modes
- **WHEN** a recording with online diarization (either mode) stops
- **THEN** the system SHALL hold per-channel timestamped embeddings covering the session's speech chunks

#### Scenario: Embeddings carry channel provenance
- **WHEN** buffered embeddings are grouped for enrollment at stop
- **THEN** each group SHALL be identifiable by channel and pipeline speaker identity, and groupings SHALL never span both channels