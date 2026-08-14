## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using polyvoice ONNX models (powerset segmentation, speaker embedding extraction, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio
- **THEN** the system runs segmentation, embedding extraction, and clustering, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use polyvoice components (streaming pipeline with a polyvoice ONNX embedder and AHC clustering) with no sherpa-onnx code or models in any diarization path

#### Scenario: Embedding extraction uses multiple cores
- **WHEN** offline diarization runs on a meeting with more than one detected speech segment
- **THEN** the system SHALL extract speaker embeddings using a batched, multi-session ONNX inference path that utilizes more than one CPU core

#### Scenario: Stereo channels diarize in parallel
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL run the microphone-channel and system-channel diarization passes concurrently, not sequentially

#### Scenario: Long recordings process without unbounded memory growth
- **WHEN** offline diarization runs on a recording longer than the configured chunk threshold
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

## ADDED Requirements

### Requirement: Batch embedding extraction
The system SHALL extract speaker embeddings from all detected segments in a batch rather than one segment at a time.

#### Scenario: Batch embed call used
- **WHEN** the diarization pipeline has N detected speech segments after segmentation
- **THEN** the system SHALL invoke the embedder's batch interface with all N segment audio slices and receive N embeddings in one coordinated call

#### Scenario: Batch extraction preserves ordering
- **WHEN** embeddings are produced by the batch call
- **THEN** the i-th returned embedding SHALL correspond to the i-th input segment so that clustering and transcript matching remain correct

### Requirement: Configurable concurrency and memory mode
The system SHALL expose settings that control diarization concurrency and memory usage.

#### Scenario: Default auto mode
- **WHEN** diarization settings are at their default values
- **THEN** the system SHALL use an "auto" memory mode that balances speed and RAM, selecting a moderate ONNX session pool size and enabling chunking only for long recordings

#### Scenario: Fast mode maximizes concurrency
- **WHEN** the user selects the "fast" memory mode
- **THEN** the system SHALL use the maximum recommended number of ONNX sessions and disable chunked processing unless the recording exceeds a hard safety threshold

#### Scenario: Low-memory mode
- **WHEN** the user selects the "low memory" memory mode
- **THEN** the system SHALL limit the ONNX session pool to at most two sessions and always process recordings in chunks

#### Scenario: Custom max session count
- **WHEN** the user sets a specific maximum session count
- **THEN** the system SHALL cap the embedder and segmenter session pools at that value, overriding the auto-derived default

### Requirement: Chunked offline diarization
The system SHALL support processing long recordings in overlapping temporal chunks to bound memory usage.

#### Scenario: Chunking triggered by duration
- **WHEN** a channel's duration exceeds the configured chunk duration
- **THEN** the system SHALL split the channel into contiguous chunks with a fixed overlap, run segmentation and embedding on each chunk independently, and discard each chunk's audio before processing the next

#### Scenario: Overlap prevents boundary artifacts
- **WHEN** chunks are created
- **THEN** adjacent chunks SHALL overlap by at least 5 seconds so that speaker turns crossing a chunk boundary are captured in full within at least one chunk

#### Scenario: Global clustering across chunks
- **WHEN** all chunks have been processed and their embeddings accumulated
- **THEN** the system SHALL run a single clustering pass over the combined set of embeddings so that the same speaker receives the same label across chunk boundaries

### Requirement: Performance observability
The system SHALL log stage-level timing and peak memory information during offline diarization.

#### Scenario: Stage timing logged
- **WHEN** offline diarization completes
- **THEN** the system SHALL log elapsed time for decode, segmentation, embedding, clustering, and transcript matching stages

#### Scenario: Memory usage logged
- **WHEN** offline diarization completes
- **THEN** the system SHALL log peak working-set memory (or best-available proxy) and the final number of segments, embeddings, and speakers

#### Scenario: Regression warning
- **WHEN** a diarization run takes longer than a configurable threshold or exceeds a memory threshold
- **THEN** the system SHALL emit a warning log with the recorded metrics
