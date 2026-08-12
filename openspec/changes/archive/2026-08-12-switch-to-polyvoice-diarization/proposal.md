## Why

Meetily currently runs **two** speaker diarization engines: sherpa-onnx for offline batch diarization (`audio/diarization.rs`, pyannote segmentation + 3D-Speaker embedding + fast clustering) and polyvoice for online diarization (`audio/online_diarization.rs`, whose `SherpaEmbedder` wraps the same sherpa-onnx embedding model). Maintaining sherpa-onnx means a heavy native dependency (`sherpa-onnx-sys` downloads prebuilt shared libraries at build time, and its ONNX Runtime conflicts forced the `shared`-feature workaround), two model download flows (tar.bz2 archive extraction + a ~26 MB 3D-Speaker model), and two engines whose speaker labels can disagree on the same audio. polyvoice (Rust-native, MIT) now covers the full diarization stack — powerset segmentation, WeSpeaker/CAM++ embedding, AHC clustering — so sherpa-onnx can be removed entirely and polyvoice becomes the single engine for both offline and online paths.

## What Changes

- **BREAKING** Remove the `sherpa-onnx` dependency (Cargo.toml, Cargo.lock, and the `sherpa-onnx-sys` build-time prebuilt-DLL download). No sherpa-onnx code, models, or spike tests remain.
- **BREAKING** Upgrade `polyvoice` from BYO-embedder mode to its full stack: features `onnx`, `download`, `segmentation`, `embedder`, `clusterer`. `ort` pin moves from `=2.0.0-rc.10` to `=2.0.0-rc.12` (polyvoice's requirement); the app's ort/ndarray usage in `audio/vad.rs` and `parakeet_engine/model.rs` is adapted if the rc.10→rc.12 API drifted.
- **Offline path** (`audio/diarization.rs`): replace sherpa `OfflineSpeakerDiarization` (segmentation + embedding + clustering in one call) with explicit polyvoice stages — `PowersetSegmenter` (same pyannote segmentation-3.0 family, INT8), a polyvoice ONNX embedder, and `AhcClusterer` — keeping the existing per-channel orchestration, segment-to-transcript matching, and `MIC_SPEAKER_NN`/`SPEAKER_NN` labeling.
- **Online path** (`audio/online_diarization.rs`): replace the `SherpaEmbedder` (sherpa-onnx `SpeakerEmbeddingExtractor`) with polyvoice's native ONNX embedder adapter (same `Embedder` trait the `StreamingPipeline` already consumes). No pipeline/recording changes.
- **Model management**: replace the custom tar.bz2 pyannote archive + 3D-Speaker downloads with polyvoice `ModelRegistry` (SHA-256 + minisign verified): `powerset_int8` (~1.6 MB) segmenter + `resnet34_int8` (~6.8 MB) embedder. The `check_diarization_models` / `download_diarization_models` commands and the settings panel stay; progress reporting becomes per-model (coarser). Stale legacy model files are cleaned up on first check.
- **Removals**: `tar` and `bzip2` dependencies (only used for diarization model extraction), sherpa-specific spike tests, and the old model-download helper code.
- **No behavior change** to recording, live transcription, transcript matching, speaker ID namespaces, or the frontend diarization UX (modes, settings, progress events, labels).

## Capabilities

### New Capabilities
<!-- None: this is an engine substitution, not a new capability. -->

### Modified Capabilities
- `speaker-diarization`: The diarization pipeline now runs entirely on polyvoice ONNX models (powerset segmentation + WeSpeaker ResNet34 embedding + AHC clustering) instead of sherpa-onnx; model management moves to the polyvoice ModelRegistry with verified downloads of `powerset_int8` + `resnet34_int8` and per-model progress reporting.

## Impact

- **Affected code**: `audio/diarization.rs` (offline engine swap + model download rework), `audio/online_diarization.rs` (embedder swap), `audio/vad.rs` + `parakeet_engine/model.rs` (ort rc.12 migration, only if API drift), `Cargo.toml`/`Cargo.lock` (deps)
- **Removed dependencies**: `sherpa-onnx` (+ `sherpa-onnx-sys` build-time DLL downloads, `bzip2`/`tar` used for model extraction, second `ureq` copy)
- **Changed dependencies**: `polyvoice` feature set (onnx, download, segmentation, embedder, clusterer), `ort` `=2.0.0-rc.12`, `ndarray` (0.16→0.17, required by ort rc.12's ndarray feature)
- **Models**: users re-download ~8.4 MB (vs ~26 MB previously); old pyannote/3D-Speaker files are stale and cleaned up
- **Risks**: ort rc.10→rc.12 API drift in the Parakeet/VAD stack (verified first, scoped to two files); polyvoice 0.x API pinned at `=0.17.0`; short-window embedding quality of ResNet34 vs the old 3D-Speaker in Fast mode (spike test before switching)
