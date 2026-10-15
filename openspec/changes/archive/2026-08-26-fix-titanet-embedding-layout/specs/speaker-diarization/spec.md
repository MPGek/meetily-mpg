## ADDED Requirements

### Requirement: TitaNet embedding input layout correctness

The system's offline diarization pipeline SHALL produce TitaNet-Large (192-d, `titanet_large`) embeddings using the input layout the bundled ONNX graph expects (`audio_signal` dimension 1 is 80 mel bins). The fbank front-end remains 80-bin log-mel at 16 kHz, but the tensor handed to ONNX for TitaNet SHALL present mel as dimension 1 (e.g. `[B, 80, T]` or model-equivalent), not the WeSpeaker `[B, T, 80]` ordering used by the generic `FbankOnnxExtractor`. The requirement applies to both batched and per-segment fallback embedding calls.

#### Scenario: Short and long segments embed without layout error

- **WHEN** offline diarization runs on a meeting whose segmentation produces segments of varied duration (e.g. from <200 ms to >8 s)
- **THEN** no segment SHALL fail with `Got invalid dimensions for input: audio_signal index:1 Got:<T> Expected:80`, and embeddings SHALL be returned for all segments that are at least one fbank window long

#### Scenario: Batched layout matches per-segment layout

- **WHEN** the pipeline embeds N segments via the batch interface and via sequential single-segment calls on the same audio
- **THEN** both paths SHALL produce N embeddings (order-preserving) and SHALL use the same Mel-as-dim-1 layout, so batch and fallback do not diverge

#### Scenario: WeSpeaker contract unchanged for non-TitaNet paths

- **WHEN** a non-TitaNet ONNX model is used with the same fbank (if ever re-enabled for testing)
- **THEN** that model SHALL continue to receive `[B, T, 80]` as documented by `polyvoice::fbank_onnx`, and the TitaNet transpose SHALL NOT apply to it

### Requirement: Diarization surfaces embedding failure instead of silent empty success

When embedding extraction produces zero valid embeddings (all batch and per-segment attempts fail), offline diarization SHALL NOT report `segments_labeled=0, speakers=0, status=complete`. It SHALL treat the run as a failure: set `diarization_status=failed` on the meeting, emit a `diarization-progress` event with `status=failed`, and return an error that preserves the underlying ONNX/layout detail.

#### Scenario: All-embeddings-failed is a failure

- **WHEN** segmentation finds speech segments but every embedding attempt errors
- **THEN** the run SHALL end with `status=failed`, SHALL NOT write empty cluster caches, and SHALL surface a message containing `audio_signal` / layout context rather than "Labeled 0 segments from 0 speakers"

#### Scenario: Partial failure still clusters the valid subset

- **WHEN** some segments fail embedding but at least one valid embedding remains
- **THEN** the run SHALL cluster only the valid subset, SHALL persist only those embeddings' caches, and SHALL still report success with the count of labeled segments

### Requirement: Offline TitaNet recognition stays model-tagged

Offline diarization's post-clustering recognition and cache persistence SHALL remain tagged `titanet_large` (192-d) and thresholds `0.52` (clustering) / `0.68` (recognition) as already specified for the enhanced-only family. This delta does not change thresholds, only enforces that embeddings reaching clustering were produced with the correct layout.

#### Scenario: Centroids are 192-d TitaNet vectors

- **WHEN** offline diarization completes successfully after this fix
- **THEN** each persisted centroid in `meeting_speakers` SHALL be 192-dimensional, `model='titanet_large'`, and cosine-similarity against enrolled TitaNet prototypes SHALL be meaningful (not a layout-corrupted vector)
