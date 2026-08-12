## Context

Meetily's current speaker diarization (`audio/diarization.rs`) operates offline: after recording stops, it re-reads the entire audio file, runs PyAnnote segmentation + 3D-Speaker embedding extraction via sherpa-onnx's `OfflineSpeakerDiarization` API, clusters with `FastClusteringConfig`, then matches segments to transcripts by temporal overlap. This duplicates audio processing already done during capture.

The recording pipeline (`audio/pipeline.rs`) already processes audio in real time: VAD (Silero v6 ONNX), resampling, noise reduction, normalization, and live transcription (Parakeet/Whisper). Adding diarization to this pipeline eliminates redundant audio re-processing.

sherpa-onnx v1.13.4 (already a dependency) provides the offline diarization API used today. **Online diarization uses the `polyvoice` crate (v0.17.0) instead**: it ships a streaming pipeline (`streaming::StreamingPipeline`), an ONNX embedding extractor (`embedder::ERes2NetV2Extractor`), an agglomerative clusterer (`clusterer::AhcClusterer`), and a verified model registry (`models::ModelRegistry`, INT8 pair ~8.4 MB, MIT, auto-download). Rationale: the spike proved sherpa-onnx exposes no streaming diarization API in v1.13.4 or v1.13.5 (see Open Questions). polyvoice runs on CPU via `ort` (the same runtime family the app's Silero VAD already uses; resolves to `ort 2.0.0-rc.12`).

**Key constraint**: The recording pipeline already runs 2-3 ONNX models simultaneously (Silero VAD, Parakeet encoder/decoder/joint) on CPU. Adding segmentation + embedding models risks CPU saturation on lower-end machines.

## Goals / Non-Goals

**Goals:**
- Eliminate the post-recording diarization wait by processing audio during recording
- Support two operational modes: Full Streaming (real-time speaker labels) and Hybrid (embeddings during recording, clustering at stop)
- Use the `polyvoice` crate (Rust-native, MIT) — new ~8.4 MB INT8 models auto-downloaded via its registry; no pyannote/HF gating
- Fall back gracefully to offline diarization if online mode fails
- User-selectable mode via diarization settings

**Non-Goals:**
- Real-time speaker label streaming to frontend in Fast mode (initially; speaker labels are computed but assigned to DB transcripts at recording stop — this avoids changing the frontend `transcript-update` event schema)
- GPU-accelerated embedding extraction (CPU-only, matching existing diarization)
- Speaker count auto-detection improvements (AhcClusterer already handles this)
- Overlapping speech diarization (polyvoice's overlap resegmentation applies at stop; not streamed live)

## Decisions

### Decision 1: Both modes write speaker labels at recording stop (not live)

**Chosen**: Speaker labels are assigned to transcripts at recording stop, even in Fast mode. During recording, Fast mode runs the full streaming diarization pipeline but buffers speaker segment assignments internally. When recording stops, the buffered assignments are flushed through the same `update_transcript_speaker` DB path as offline diarization.

**Rationale**: The frontend currently expects `speaker: None` on `transcript-update` events during recording. Changing this would require a new event type, streaming UI updates for speaker labels, and handling of label reassignment (early clustering may flip speaker indices). Deferring live UI to a follow-up change keeps scope manageable.

**Alternative considered**: Emit `speaker-assigned` events during recording with tentative labels. Rejected because clustering is unstable early in a meeting — speaker indices may reassign as more audio arrives, causing confusing UI flicker.

### Decision 1a: Speaker assignments reach the DB via the recording save (not a post-hoc UPDATE at stop)

**Chosen**: At stop, Rust computes speaker assignments (`sequence_id -> "MIC_SPEAKER_NN" | "SPEAKER_NN"`) from the online processor's final turns and carries them in the `recording-stopped` event payload. The frontend attaches them to its transcript objects (by `sequence_id`) before calling `api_save_transcript`; `TranscriptsRepository::save_transcript` persists the `speaker` column (currently ignored) and sets `diarization_status = 'complete'` when any segment carries a speaker. Offline diarization's `update_transcript_speaker` remains the path for re-analysis/fallback.

**Rationale**: The meeting + transcripts are not in SQLite at Rust stop time — the frontend saves them after `recording-stopped` (see `stop_recording` in `recording_commands.rs`: "Database save removed - frontend will handle this after receiving all transcripts"). A stop-time `UPDATE` would race the save. Persisting during the save transaction is atomic, reuses the existing `TranscriptSegment.speaker` field (already in the API schema), and keeps one write path. `DiarizationGuard`-style concurrency protection still applies on the Rust side (only one online session at a time).

**Alternative considered**: New `apply_speaker_assignments(meeting_id, …)` command invoked by the frontend right after save. Rejected — transcripts carry no `sequence_id` in the DB today, so matching would require a schema change; attaching before save is simpler and atomic.

### Decision 2: Embedding extraction runs on the same audio path as VAD (16 kHz mono)

**Chosen**: Tap into the VAD-processed audio stream at 16 kHz mono, the same used by transcription. Embedding extraction requires 16 kHz input; reusing the already-resampled stream avoids a second resampling pass.

**Rationale**: The VAD already resamples from capture rate (varies) to 16 kHz. Sending this to both transcription and embedding avoids redundant resampling. The 3D-Speaker model expects 16 kHz.

### Decision 3: Polyvoice is the online diarization engine (BYO-embedder mode)

**Chosen**: Use the `polyvoice` crate (v0.17.0, MIT) in **BYO-embedder mode** (`default-features = false` + `clusterer`; the `onnx` feature is NOT enabled):
- Fast mode: `polyvoice::streaming::StreamingPipeline` — real-time windowing + speaker cache (arrival-order with stability hysteresis), `LatencyPreset::Balanced` (~1.5s window).
- Efficient mode: embed per VAD segment during recording; `polyvoice::clusterer::AhcClusterer` (agglomerative, auto-threshold) clusters at stop.
- Embedder: a `SherpaEmbedder` wrapper implementing polyvoice's `Embedder` trait around the **existing sherpa-onnx 3D-Speaker model** (`3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx`, 512-dim) — already downloaded for offline diarization. Verified by spike probe: embeddings available from ~0.25s windows.

**Rationale**: Two blockers killed the full-ONNX polyvoice path: (1) sherpa-onnx v1.13.4/1.13.5 expose NO streaming `SpeakerDiarization` API (spike finding, open question 1), and (2) polyvoice's `onnx` feature requires `ort >= 2.0.0-rc.12` + `ndarray 0.17`, which is semver-incompatible with the app's Parakeet/VAD stack (`ort 2.0.0-rc.10` + `ndarray 0.16` — the unified build fails to compile in the Parakeet engine). BYO mode avoids the dependency conflict entirely: **ort is pinned to `=2.0.0-rc.10`** and the app's transcription core is untouched. The existing offline diarization (sherpa-onnx `OfflineSpeakerDiarization`) stays as the fallback path.

**Alternative considered**: (a) Migrating the app's ort/ndarray stack (Parakeet engine, Silero VAD, device fallback, summary sidecar) to rc.12/0.17 to unlock polyvoice's own ONNX models — rejected: large, risky change to the transcription core with no runtime verification possible here. (b) Hand-rolling clustering on sherpa-onnx embeddings without polyvoice — polyvoice's tested AHC + streaming cache is superior.

### Decision 3a: Efficient mode buffers embeddings (not raw audio) and clusters with AhcClusterer at stop

**Chosen**: Per spec: extract one 512-dim embedding per VAD speech segment during recording (sherpa-onnx 3D-Speaker via `SherpaEmbedder`), buffer `(start, end, embedding)` per channel, and at recording stop run `AhcClusterer` (auto threshold selection; `max_speakers` from settings as ceiling) on each channel's buffer. No clustering CPU during recording; stop-side clustering is sub-100ms for typical meetings.

**Rationale**: Matches the spec scenarios ("extract embeddings during recording", "cluster at stop") and keeps recording-time CPU to a single embedding model on speech segments only (comparable to VAD cost).

### Decision 3b: StreamingPipeline uses the app's existing VAD, not polyvoice's

**Chosen**: `StreamingPipeline` is fed the pipeline's VAD-merged 16 kHz speech segments (the same chunks already routed to transcription via the `embedding_sender`). Internally polyvoice still runs its own speech-state machine; a low-threshold `EnergyVad` wrapper is used so already-VAD-filtered audio is never dropped. No duplicate Silero VAD session.

**Rationale**: Avoids running a second VAD model on the same audio. The pipeline's Silero VAD segmentation is the established speech gate (spec: "VAD-detected speech segments").

### Decision 4: New `embedding_sender` channel in `AudioPipelineManager`

**Chosen**: Add a second `mpsc::UnboundedSender<AudioChunk>` called `embedding_sender` to `AudioPipelineManager`, passed to the pipeline alongside `transcription_sender`. The pipeline sends VAD-filtered audio chunks to both channels.

**Rationale**: Clean separation — transcription and diarization are independent consumers of the same audio stream. If diarization is disabled or fails, the transcription path is unaffected.

**Alternative considered**: Sending diarization from the transcription worker (reusing processed audio). Rejected because it couples transcription and diarization; if transcription slows, diarization would also stall.

### Decision 5: Mode stored in `localStorage` (frontend), not in DB

**Chosen**: Store `diarizationMode: "fast" | "efficient" | "off"` in the frontend's existing `diarization.ts` settings (`localStorage`), alongside `enabled`, `autoRun`, and `maxSpeakers`.

**Rationale**: Consistent with existing diarization settings pattern. No DB migration needed. Mode is a UX preference, not meeting-specific data.

### Decision 6: Online diarization processes mic and system channels separately

**Chosen**: The online processor keeps the two channels fully separate, mirroring offline per-channel diarization:
- Efficient mode: two `EmbeddingBuffer` instances keyed by chunk `device_type`; clustering runs per buffer at stop; mic clusters map to `MIC_SPEAKER_NN`, system clusters to `SPEAKER_NN`.
- Fast mode: two `SpeakerDiarization` instances, each fed only its own channel's audio.

**Rationale**: Audio chunks on the `embedding_sender` already carry `device_type` (the pipeline runs separate VAD processors per device), so channel separation costs nothing structurally. Remote participants are all mixed into the system channel and can only be distinguished if that channel is diarized independently — this matches the `diarization-per-channel` change, keeping labels consistent between offline and online paths.

**Alternative considered**: A single mixed-stream pipeline. Rejected — indistinguishable remote speakers, and the offline fix already establishes the namespaced-ID scheme.

**Interaction**: The `diarization-per-channel` change (offline) defines the ID scheme `MIC_SPEAKER_NN` / `SPEAKER_NN` and per-source matching; this change SHALL reuse it so offline re-analysis after an online run produces identical labels.

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| **CPU overload on low-end machines in Fast mode**: segmentation + embedding + VAD + transcription all running simultaneously | Fast mode gated behind explicit user selection; Efficient mode is the safe default (embedding-only during recording). Frontend shows estimated CPU impact in settings tooltip. |
| **polyvoice is beta (0.x) and its public API may break between minor versions** | Pin the exact version (`=0.17.0`) in Cargo.toml; the integration is isolated in `audio/online_diarization.rs` so an upgrade touches one file. |
| **Embedding model missing at recording start** | `SherpaEmbedder::new` fails fast with the same message style as offline diarization ("Download models in Settings"); the processor reports the error and recording falls back to offline diarization (auto-run path unchanged). |
| **Streaming quality with the 3D-Speaker embedder is unproven on real meetings** | polyvoice's cache/clustering math is model-agnostic; spike verified the pipeline runs end-to-end. If quality disappoints, Efficient mode remains, and the offline fallback always works. |
| **Embedding buffer memory growth**: In Efficient mode, embeddings accumulate per speech segment. A 2-hour meeting with 300 segments × 512-dim embedding = ~600KB — negligible. | N/A — memory is not a concern. |
| **Speaker index instability in Fast mode**: the arrival-order cache emits provisional labels that may flip before stabilizing | Deferred to internal buffer (`stable: true` turns only are kept); only final labels are written to DB at stop, avoiding UI confusion. |

## Open Questions

1. **Does sherpa-onnx v1.13.4 `SpeakerDiarization` accept the exact same ONNX model files used for offline diarization?** — **RESOLVED (spike, 2026-08-12): N/A — the API does not exist.** The Rust crate (checked in v1.13.4, the pinned version, and v1.13.5, the latest published release) exposes **only** `OfflineSpeakerDiarization`. There is no streaming/online `SpeakerDiarization` type, no accept-stream/flush diarizer, and no standalone `FastClustering` binding. **Decision (change owner, 2026-08-12): switch the online engine to `polyvoice` v0.17.0 in BYO-embedder mode** (streaming pipeline + AHC clusterer + the existing sherpa-onnx 3D-Speaker model as embedder), keeping sherpa-onnx for the offline fallback path. Polyvoice's own ONNX path was rejected: it forces `ort >= rc.12` + `ndarray 0.17`, which breaks the app's Parakeet/VAD stack (ort is pinned to `=2.0.0-rc.10`).

2. **What is the minimum chunk size for `SpeakerDiarization` streaming?** — **RESOLVED (spike): moot** — the sherpa-onnx streaming API does not exist. For polyvoice: `StreamingPipeline::feed(&[f32])` accepts arbitrary-length 16 kHz mono chunks and buffers them internally against its window/hop geometry (`LatencyPreset::Balanced` ≈ 1.5s window); `flush()` returns remaining turns. A standalone probe (`embed-probe`) verified the sherpa 3D-Speaker embedder returns 512-dim embeddings from ~0.25s windows, and that the StreamingPipeline emits stable turns end-to-end with it.

3. **Should we add a CPU usage indicator in the UI when Fast mode is active?** — Deferred. Could be a follow-up improvement.

4. **Efficient mode clustering at stop** — **RESOLVED**: `polyvoice::clusterer::AhcClusterer` (agglomerative; `new(max_speakers)` auto-threshold) clusters the buffered embeddings at stop — the "sub-100ms clustering" property is preserved, no custom clustering code needed. Verified by probe on real embeddings.
