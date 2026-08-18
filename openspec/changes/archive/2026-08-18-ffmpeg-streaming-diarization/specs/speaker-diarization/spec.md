## ADDED Requirements

### Requirement: Streaming audio decode via ffmpeg
The system SHALL decode, resample, and channel-split recorded audio for offline diarization by streaming it through a spawned ffmpeg process on stdout, rather than decoding the entire file into memory. The system SHALL consume the stream in bounded in-memory chunks so peak memory does not scale with recording length, and SHALL fall back to the existing Symphonia decoder only when ffmpeg is unavailable.

#### Scenario: Stereo recording decoded and split via ffmpeg
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL spawn ffmpeg to decode the file, resample each channel to 16kHz, and emit the microphone (left) and system (right) channels as separate mono streams, so the Rust pipeline receives per-channel 16kHz samples without ever holding the full decoded interleaved buffer

#### Scenario: Decode memory is bounded
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL hold only a single in-memory audio chunk per channel at a time during segmentation and embedding, and SHALL NOT materialize a full-length decoded sample buffer

#### Scenario: ffmpeg unavailable falls back to Symphonia
- **WHEN** the ffmpeg executable cannot be located
- **THEN** the system SHALL decode the audio with the existing Symphonia path and continue diarization, at the cost of higher peak memory

### Requirement: Fixed diarization concurrency profile
The system SHALL use a single fixed concurrency and memory profile for offline diarization with no user-facing memory-mode or session-count settings. The system SHALL size the segmenter and embedder ONNX session pools to the smaller of 8 or 75% of the logical CPU core count (rounded up, minimum 1), and SHALL always process recordings in overlapping chunks.

#### Scenario: Session pool sized from core count
- **WHEN** offline diarization runs on a machine with N logical CPU cores
- **THEN** the segmenter and embedder session pools SHALL each contain `min(8, ceil(0.75 × N))` sessions, never exceeding 8

#### Scenario: Single-core floor
- **WHEN** offline diarization runs on a machine with one logical CPU core
- **THEN** the session pools SHALL each contain exactly 1 session

#### Scenario: No memory-mode setting
- **WHEN** the user opens the diarization settings
- **THEN** the system SHALL NOT present a memory-mode selector or a session-count override, and diarization SHALL always use the fixed profile

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
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

### Requirement: Per-channel offline diarization
The system SHALL process the microphone (left) and system (right) channels of a stereo recording independently during offline diarization, splitting the audio into two mono streams before segmentation.

#### Scenario: Stereo recording diarized per channel
- **WHEN** offline diarization runs on a stereo recording (2 channels, left=microphone, right=system)
- **THEN** the system SHALL produce a 16kHz microphone stream and a 16kHz system stream, resample each to 16kHz independently, and run the diarization pipeline once per channel, reusing a single diarizer instance loaded once

#### Scenario: Silent channel produces no speakers
- **WHEN** one channel of a stereo recording contains no speech
- **THEN** the system SHALL produce no speaker segments for that channel and its transcripts SHALL remain unlabeled rather than failing the whole run

### Requirement: Chunked offline diarization
The system SHALL process recordings in overlapping temporal chunks to bound memory usage, using a fixed chunk duration.

#### Scenario: Chunking triggered by duration
- **WHEN** a channel's duration exceeds the fixed chunk duration
- **THEN** the system SHALL split the channel into contiguous chunks with a fixed overlap, run segmentation and embedding on each chunk independently, and discard each chunk's audio before processing the next

#### Scenario: Overlap prevents boundary artifacts
- **WHEN** chunks are created
- **THEN** adjacent chunks SHALL overlap by at least 5 seconds so that speaker turns crossing a chunk boundary are captured in full within at least one chunk

#### Scenario: Global clustering across chunks
- **WHEN** all chunks have been processed and their embeddings accumulated
- **THEN** the system SHALL run a single clustering pass over the combined set of embeddings so that the same speaker receives the same label across chunk boundaries

## REMOVED Requirements

### Requirement: Configurable concurrency and memory mode
**Reason**: The auto/fast/low-memory modes and the max-session override asked users to choose a memory/performance tradeoff that should be an internal decision. A single fixed profile (pool size `min(8, ceil(0.75 × cores))`, always chunk) replaces them.

**Migration**: Remove the `diarizationMemoryMode` and `diarizationMaxSessions` keys from localStorage and the `memory_mode`/`max_sessions` keys from the `diarization-settings.json` store. Existing stored values are ignored with no migration required.
