## MODIFIED Requirements

### Requirement: Speaker diarization pipeline
The system SHALL provide a speaker diarization pipeline using the enhanced polyvoice ONNX model set (pyannote `segmentation-3.0` segmentation, TitaNet-Large speaker embedding, and agglomerative clustering) that processes recorded audio and assigns speaker labels to transcript segments. The pipeline SHALL resolve its segmentation and embedding models through the 3-location fallback chain and SHALL utilize multiple CPU cores during embedding extraction, process stereo channels concurrently, and cap memory growth for long recordings. Clustering SHALL honor runtime-configurable merge parameters and an always-enforced speaker-count ceiling as specified by the diarization-param-tuning capability, with built-in defaults chosen by the measured sweep protocol.

#### Scenario: Successful diarization of a meeting
- **WHEN** diarization is triggered for a saved meeting with valid audio and the enhanced models are present in any fallback location
- **THEN** the system runs segmentation, embedding extraction, and clustering via the resolved model directory, and assigns `speaker` values ("SPEAKER_00", "SPEAKER_01", etc.) to matching transcript segments

#### Scenario: Diarization handles missing audio file
- **WHEN** diarization is triggered but the meeting has no audio file
- **THEN** the system SHALL return an error with a clear message and set `diarization_status` to "failed"

#### Scenario: Online diarization uses the same engine family
- **WHEN** online diarization runs during recording
- **THEN** the system SHALL use the same enhanced model set (segmentation-3.0, TitaNet-Large, AHC clustering) resolved via the shared 3-location fallback with no sherpa-onnx code or models in any diarization path

#### Scenario: Embedding extraction uses multiple cores
- **WHEN** offline diarization runs on a meeting with more than one detected speech segment
- **THEN** the system SHALL extract speaker embeddings using a batched, multi-session ONNX inference path that utilizes more than one CPU core

#### Scenario: Stereo channels diarize in parallel
- **WHEN** offline diarization runs on a stereo recording
- **THEN** the system SHALL run the microphone-channel and system-channel diarization passes concurrently, not sequentially

#### Scenario: Long recordings process without unbounded memory growth
- **WHEN** offline diarization runs on a recording of any length
- **THEN** the system SHALL process each channel in overlapping chunks, accumulating only embeddings and segment metadata between chunks, so peak memory does not grow linearly with recording duration

#### Scenario: Offline clustering respects the speaker-count ceiling
- **WHEN** offline diarization clusters a channel's embeddings
- **THEN** the number of distinct speaker labels in the result does not exceed the effective ceiling (user max-speakers when set, otherwise the configured default ceiling), and the clustering merge threshold and gap-merge window come from the resolved runtime parameters
