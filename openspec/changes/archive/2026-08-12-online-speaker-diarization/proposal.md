## Why

Meetily's existing speaker diarization runs as a batch post-processing step after recording stops — it re-reads the entire audio file, re-runs segmentation and embedding extraction, then clusters. This duplicates work (audio was already processed during recording) and introduces a delay before speaker labels appear. Running diarization online during recording eliminates both the post-recording wait and the redundant audio re-processing, delivering speaker-labeled transcripts in near-real-time.

## What Changes

- **Two online diarization modes**, user-selectable via settings, powered by the **`polyvoice`** Rust crate (v0.17.0, MIT) in BYO-embedder mode:
  - **Fast mode** (Path A — Full Streaming): polyvoice `StreamingPipeline` runs windowed embedding + incremental speaker caching continuously during recording. Speaker labels appear at recording stop without offline reprocessing. For high-performance machines.
  - **Efficient mode** (Path B — Hybrid): Speaker embeddings extracted during recording (lightweight), clustering deferred to recording stop (sub-100ms AHC). Speaker labels appear near-instantaneously when recording ends, without re-processing the audio file. For CPU-constrained machines.
- **New `audio/online_diarization.rs` module**: `OnlineDiarizationProcessor` struct that receives audio chunks from the existing pipeline, manages embedding extraction and (optionally) incremental streaming diarization
- **Pipeline extension**: An `embedding_sender` channel added alongside the existing `transcription_sender` in `AudioPipelineManager`, routing audio chunks to the online diarization processor
- **Settings UX**: New "Diarization Mode" dropdown in diarization settings (Fast / Efficient / Off)
- **Models**: Reuses the existing sherpa-onnx 3D-Speaker embedding model (already downloaded for offline diarization) as the polyvoice embedder — no new model downloads, no new ONNX runtime (polyvoice runs in BYO-embedder mode; ort stays pinned at `2.0.0-rc.10`)
- **Recording stop integration**: On recording stop, either flush buffered streaming turns (Fast mode) or run AHC clustering on buffered embeddings (Efficient mode), then persist speaker labels per transcript; the existing offline `update_transcript_speaker` path remains for fallback and re-analysis
- **Per-channel speaker separation**: Mic and system audio are diarized independently (two embedding buffers / two streaming instances), producing the same namespaced IDs as the offline `diarization-per-channel` change — `MIC_SPEAKER_NN` for local, `SPEAKER_NN` for remote speakers
- **Fallback**: If online processing is unavailable or errors, gracefully fall back to existing offline diarization pipeline

## Capabilities

### New Capabilities
- `online-speaker-diarization`: Real-time speaker diarization processing during recording with two operational modes (full streaming and hybrid embedding+deferred clustering), user mode selection, and graceful fallback to offline diarization

### Modified Capabilities
- `speaker-diarization`: The diarization system now supports an online/streaming mode in addition to offline batch mode. The `DiarizationGuard`, progress events, and model paths are shared between online and offline paths. DB update path (`update_transcript_speaker`) is reused without change.
- `audio-engine`: The recording pipeline is extended with a parallel audio routing channel for embedding extraction. The `AudioPipelineManager` gains an optional `embedding_sender`, and the recording stop flow integrates diarization completion (clustering or label retrieval) before the final save.

## Impact

- **Affected code**: New `audio/online_diarization.rs` (~300 lines); modified `audio/pipeline.rs` (add `embedding_sender` channel), `audio/recording_commands.rs` (integrate online diarization into start/stop flow), `audio/mod.rs` (module declaration); frontend `diarization.ts` (new mode setting), settings UI (mode dropdown), `recordingService.ts` (pass mode to start command)
- **New dependency**: `polyvoice = "=0.17.0"` (MIT), BYO-embedder mode (`default-features = false` + `clusterer`) — no new ONNX runtime; the existing sherpa-onnx 3D-Speaker model serves as the embedder (512-dim, verified from ~0.25s windows)
- **No breaking changes** — offline diarization remains available as default/fallback; new channels and processors are optional gated by mode setting. `ort` is pinned to `=2.0.0-rc.10` to protect the Parakeet/VAD stack from pre-release drift
- **Binary size**: polyvoice adds pure-Rust clustering/cache code — negligible; no new model files
- **Memory**: ~50-100MB additional for embedding extraction buffer during recording (single meeting), freed on stop
- **CPU risk mitigation**: Mode selection lets users choose between throughput (Fast) and safety (Efficient); Efficient mode adds only embedding extraction cost during recording (~comparable to VAD)
