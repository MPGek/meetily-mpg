## Why

Diarization accuracy is the weakest link in the transcription stack: the bundled polyvoice ONNX models (powerset segmentation v1.6-era, WeSpeaker ResNet34 embeddings) are markedly less accurate than current options (pyannote segmentation-3.0, TitaNet-Large embeddings), and speaker assignment operates at whole-segment granularity, so a transcript chunk spanning a speaker change is labeled with a single speaker. Users get merged or misattributed turns in exactly the conference-call and two-speaker meetings this app targets.

## What Changes

- **Enhanced diarization model set (Route A, low risk)**: keep the existing polyvoice pipeline skeleton (chunked processing, batched embedding, AHC clustering, Fast-mode streaming pipeline) but run it against a stronger, repo-managed ONNX model set when installed:
  - Segmentation: pyannote `segmentation-3.0` ONNX (replaces `powerset_int8`).
  - Embeddings: NVIDIA TitaNet-Large ONNX, 192-dim (replaces WeSpeaker ResNet34, 256-dim).
  - The legacy polyvoice models remain bundled and are used as the automatic fallback whenever the enhanced models are not installed, so behavior never degrades for existing installs.
- **Word/token-level speaker assignment**: when the ASR engine supplies token timestamps, speaker assignment uses them to refine transcript ownership, splitting a stored transcript segment that spans detected speaker changes into separate speaker-labeled rows per contiguous speaker block (including multi-speaker segments → `N` rows, one per block, each boundary validated by ≥2 contiguous tokens). Segment-level overlap matching remains the fallback.
- **Model-aware voiceprint registry**: voiceprints are already tagged with a `model` tag; recognition now compares only within the embedder model family used by the diarization run (legacy 256-dim ResNet34 vs enhanced 192-dim TitaNet), and per-family clustering thresholds are calibrated (legacy τ=0.45; TitaNet family gets its own threshold).
- **Build-time model bundling (no runtime download)**: the enhanced pair is fetched at **build time** by `build.rs` / `scripts/fetch-enhanced-models.*` from public `onnx-community/pyannote-segmentation-3.0` and `Recogment/titanet-large-onnx` (no `HF_TOKEN` required), verified (SHA-256 + minisign) and bundled as app resources; **no online downloading at runtime**. The settings panel shows **read-only** bundled status. The legacy polyvoice pair remains bundled and is the automatic fallback when the enhanced set is not bundled.

No new AI engines, no Python at runtime, no changes to the audio capture, VAD, or transcription paths. No network access at runtime for model fetching.

## Capabilities

### New Capabilities

None — this change modifies existing behaviors rather than introducing a new capability.

### Modified Capabilities
<!-- Existing capabilities whose REQUIREMENTS are changing (not just implementation).
     Each needs a delta spec file at offers the exact existing path. -->

- `speaker-diarization`: pipeline may run on the enhanced model set when installed (fallback to legacy); model management covers the enhanced pair; clustering threshold is per model family; speaker label assignment gains word/token-level refinement that splits cross-speaker transcript segments.
- `online-speaker-diarization`: Efficient mode extracts embeddings through a model-aware embedder that uses the enhanced TitaNet model when installed (ResNet34 otherwise); stop-time assignment applies word/token-level refinement when token timestamps are available.
- `speaker-identity-registry`: voiceprint storage generalizes beyond a fixed 256-dim blob to model-family-dimensioned embeddings; recognition matches only within the run's model family with per-family thresholds.

## Impact

- **Backend**: `frontend/src-tauri/src/audio/diarization.rs` (segmentation/embedder construction, clustering threshold per family, word-level assignment), `frontend/src-tauri/src/audio/online_diarization.rs` (Efficient-mode embedder abstraction), whisper worker/engine plumbing that already captures token timestamps (`whisper_engine.rs`, `worker.rs`), transcript write path used by diarization finalize (`update_transcript_speaker`), **build-time** model fetch/bundling (`build.rs` / `scripts/fetch-enhanced-models.*`) and runtime read-only verification of bundled resources (no download commands).
- **DB**: `speaker_embeddings` rows remain tagged by `model`; no schema migration required (embedding stored as blob, dimension implied by `model` tag). Transcript splitting creates additional transcript rows at speaker-change boundaries.
- **Dependencies**: `ort` (already used, CUDA + CPU EPs); `polyvoice` remains pinned (0.17.0) as the Fast-mode streaming backbone and legacy fallback; new enhanced ONNX model artifacts are versioned and verified at **build time** (SHA-256 + minisign) — `onnx-community/pyannote-segmentation-3.0` and `Recogment/titanet-large-onnx` are both public (no `HF_TOKEN`), then bundled; no runtime network.
- **Frontend**: diarization settings panel — **read-only** bundled status (no download/remove controls); transcript view rendering of split speaker blocks (existing per-block rendering already supports this).
- **Assumptions (recorded)**: enhanced models are **built-time bundled** from public ONNX mirrors; they become the default only when bundled, otherwise legacy is used; no runtime download and no `HF_TOKEN` required; Fast (streaming) mode continues to run the polyvoice streaming pipeline unchanged since its embedder is non-swappable without forking the pinned crate; token timestamps are treated as approximate alignment, not forced phoneme alignment.