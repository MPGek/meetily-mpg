## Context

Meetily's diarization stack has two engines. Offline (`audio/diarization.rs`, ~920 lines): sherpa-onnx `OfflineSpeakerDiarization` runs pyannote segmentation-3.0 (INT8, downloaded as a tar.bz2 archive) + 3D-Speaker eres2net embedding + `FastClusteringConfig` (threshold 0.5) in one object, once per channel. Online (`audio/online_diarization.rs`, ~520 lines): polyvoice v0.17.0 in BYO-embedder mode (`default-features = false`, feature `clusterer`) — `StreamingPipeline` (Fast mode) and `EmbeddingBuffer` + `AhcClusterer` (Efficient mode), both consuming a `SherpaEmbedder` that wraps sherpa-onnx's `SpeakerEmbeddingExtractor` on the same 3D-Speaker ONNX file.

Why sherpa-onnx is a burden: `sherpa-onnx = { features = ["shared"] }` pulls `sherpa-onnx-sys`, which downloads prebuilt shared libraries at build time (bzip2/tar/ureq); the ONNX Runtime it embeds conflicts with the `ort` crate (hence the `shared` feature); two model download flows (tar.bz2 extraction + a ~25 MB 3D-Speaker file); and offline/online results come from different engines.

polyvoice 0.17.0 (already pinned, MIT) covers the full stack with the `onnx`/`download`/`segmentation`/`embedder`/`clusterer` features:
- `PowersetSegmenter` (`segmentation` + `onnx`) — Segmenter trait, wraps the same `sherpa-onnx-pyannote-segmentation-3-0` model family (powerset-3.0, INT8 ~1.6 MB), 10 s window / 2 s hop, pooled sessions, batched windows.
- Embedders (`embedder` + `onnx`) — `FbankOnnxExtractor` implements `Embedder` directly; `ResNet34Adapter` (256-d), `CamPlusPlusExtractor` (512-d), `ERes2NetV2Extractor` (192-d) wrap it with per-model fbank preprocessing.
- `AhcClusterer` (`clusterer`) — already used by the online path.
- `ModelRegistry` (`download`) — manifest-driven, SHA-256 + minisign-verified downloads: `powerset_int8` (1.6 MB) + `resnet34_int8` (6.8 MB) via `ensure_for_profile(Profile::Balanced)`.

Constraint: polyvoice's `onnx` feature requires `ort >= 2.0.0-rc.12` (+ ndarray 0.17, + `download-binaries`/`tls-rustls`/`copy-dylibs` features). The app pins `ort = "=2.0.0-rc.10"` because "rc.13+ breaks the rc.10 API usage"; ort's API did drift after rc.10 (rc.13 restructured `execution_providers/` → `ep/` and reorganized tensor APIs). Direct ort consumers in the app: `audio/vad.rs` (Silero VAD: `ort::inputs`, `Session`, `TensorRef`, ndarray 0.16) and `parakeet_engine/model.rs` (Parakeet: same + `CPUExecutionProvider`).

## Goals / Non-Goals

**Goals:**
- sherpa-onnx (crate + sys + build-time DLL downloads) fully removed from the dependency tree
- polyvoice is the single diarization engine for offline and online paths, sharing one embedder model so labels are consistent
- Preserve the existing behavior surface: per-channel offline diarization, `MIC_SPEAKER_NN`/`SPEAKER_NN` namespacing, online modes (Fast/Efficient), `diarization-progress` events, `check/download_diarization_models` commands, transcript matching
- Verified model downloads via polyvoice `ModelRegistry` (checksum + signature), ~8.4 MB total

**Non-Goals:**
- Changing the frontend diarization UX (modes, settings panel, progress events, speaker labels) — labels stay generic ("Segmentation model", "Speaker embedding model")
- Adopting polyvoice's crate-root v2 `Pipeline`/VBx/resegmentation/overlap handling — per-channel orchestration stays custom; only the engines inside it change
- ort migration beyond what rc.12 requires (no GPU EPs, no API modernization of `vad.rs`/Parakeet beyond compile compatibility)
- Online-model differences: `max_speakers` hardcoded to 0 in the online path is pre-existing and untouched
- Bundling models in the installer — runtime downloads remain, matching today

## Decisions

### Decision 1: Enable polyvoice's full ONNX stack and pin ort to exactly `=2.0.0-rc.12`

**Chosen**: `polyvoice = { version = "=0.17.0", default-features = false, features = ["onnx", "download", "segmentation", "embedder", "clusterer"] }` and `ort = { version = "=2.0.0-rc.12" }` (exact pin, so Cargo cannot float to rc.13). `ndarray` direct dep moves to 0.17 (ort rc.12's `ndarray` feature uses ndarray 0.17; `TensorRef` generics tie the app's tensor code to that version). Then fix compile drift in `audio/vad.rs` and `parakeet_engine/model.rs` (expected surface: `inputs!` macro, `Session::builder`, `TensorRef` extraction, ndarray imports).

**Rationale**: polyvoice's own adapters (segmenter fbank pipeline, embedder fbank+CMVN, session pooling) are tested and DER-benchmarked; hand-rolling them would mean reimplementing fbank/CMVN preprocessing against the app's ort rc.10, which is exactly the "two engines" problem this change removes. rc.12 is the version polyvoice 0.17.0 was built and published against; the known-breaking restructure happened at rc.13 (`execution_providers/` → `ep/`, tensor API rework), so rc.12 is the smallest step that satisfies polyvoice. The exact `=` pin protects both sides from rc.13 drift.

**Alternative considered**: (a) Keep ort rc.10 and write custom `Embedder` + `Segmenter` impls around ort for the existing 3D-Speaker/pyannote models — rejected: reimplements fbank/CMVN/windowing preprocessing that polyvoice already ships, keeps two model sets, and gains no API compatibility benefit; (b) `[patch.crates-io]` a polyvoice fork relaxing the ort requirement to rc.10 — rejected: fork maintenance + untested ort API combination; (c) tract backend (`backend-tract`) — still pulls the `onnx` feature and thus ort, no gain.

**Verification-first**: Task 0 of the migration is `cargo build` + unit tests on `vad.rs`/Parakeet before any diarization work, so the ort step is de-risked in isolation.

### Decision 2: Offline path keeps custom per-channel orchestration, swaps the engine inside it

**Chosen**: `run_diarization_blocking` keeps its structure (decode → de-interleave per channel → resample 16 kHz → run engine per channel → `compute_speaker_matches`), but `create_diarizer`/`run_sherpa_diarization` are replaced by a `create_polyvoice_diarizer` that builds, once per run:
- `PowersetSegmenter::new(PowersetConfig::default().with_model_meta(meta))` — powerset-3.0 INT8, 10 s/2 s windows, batched windows (default batch 8), session pool = min(cores, 4)
- an embedder from the same model family used online (see Decision 4)
- `AhcClusterer::new(max_speakers)` (auto-threshold when -1), threshold behavior inherited from the online path

Per channel: `segmenter.segment(&samples)` → `Vec<RawSegment>` (already sorted, overlap-marked) → embed each segment's audio slice with `embedder.embed()` → `clusterer.cluster(&embeddings)` → map `(segment, cluster_idx)` into the existing `DiarizationSegment { start, end, speaker }` list → existing `compute_speaker_matches` unchanged. Overlap segments (powerset emits two speakers on one range) produce two entries for the same range; `find_best_speaker` already picks by max overlap, so no special casing.

**Rationale**: The per-channel split and namespaced IDs (`MIC_SPEAKER_NN`/`SPEAKER_NN`) are requirements from the archived per-channel change; polyvoice's crate-root v2 `Pipeline` operates on whole files/mixed streams and would lose the channel split. The polyvoice stages expose exactly the primitives the current flow needs, so the diff stays inside `audio/diarization.rs`.

**Alternative considered**: `LegacyPipeline` (v1, segmenter+embedder+vad+clusterer) — its VAD-based segmentation is not powerset-based and doesn't match the offline spec flow; v2 `Pipeline` — whole-file only, needs `resegmentation` + profile plumbing, no per-channel support.

### Decision 3: Model management moves to polyvoice ModelRegistry; commands and settings panel stay

**Chosen**: Replace the tar.bz2 extraction + 3D-Speaker download helpers with `ModelRegistry::with_cache_dir(models_dir)` rooted at the existing `<app_data_dir>/models` directory:
- `check_diarization_models` → `registry.ensure_in_cache_only` is test-only; instead check `ensure_for_profile`-style path existence via `manifest` (segmenter/embedder paths from `ProfileModels` or direct `cache_dir.join(filename)`) — i.e., verify both model files exist and validate the ONNX header (polyvoice's `build_session_with_ep` already validates headers at construction)
- `download_diarization_models` → `registry.ensure_for_profile(Profile::Balanced)` (downloads `powerset_int8` + `resnet34_int8`, SHA-256 + minisign verified)
- Progress: polyvoice's downloader has no callback; emit `diarization-model-download-progress` per model with an overall percentage (0/50/100 or per-model start/complete). Manifest sizes are known, so the frontend toast shows model name + size.
- Cleanup: on `check_diarization_models`, delete stale sherpa-era files (`model.int8.onnx`, `3dspeaker_*.onnx`) in `models_dir`.
- Remove `tar` and `bzip2` deps (their only consumer was model extraction).

**Rationale**: `ModelRegistry` gives checksum + signature verification, an embedded manifest, and idempotent `ensure` for free; keeping the command surface means zero frontend changes. `Profile::Balanced` resolves the signed production pair.

**Alternative considered**: Hand-rolled reqwest download + our own sha256 — rejected: loses minisign verification and duplicates manifest logic.

### Decision 4: One embedder model for offline and online — `resnet34_int8` (256-d)

**Chosen**: Both paths use polyvoice's `resnet34_int8` (WeSpeaker ResNet34, 256-d, VoxConverse-calibrated, static QDQ, 6.8 MB) — the polyvoice `balanced` profile default. Offline: `ResNet34Adapter::new(path, pool_size, ExecutionProvider::Cpu)`. Online: the same adapter type replaces `SherpaEmbedder` in `StreamingPipeline` (Fast) and `EmbeddingBuffer` (Efficient); the `Embedder` trait boundary is unchanged, so `online_diarization.rs` only swaps the constructor and drops the sherpa imports.

**Rationale**: One embedder guarantees offline re-analysis and online runs produce comparable labels; profile default is polyvoice's tested baseline. 256-d vs the old 512-d 3D-Speaker is a quality trade-off owned by the upstream calibration; `cam_pp_int8` (512-d, same fbank adapter) is a drop-in alternative if short-window quality in Fast mode disappoints (spike test in tasks verifies embeddings from ~0.25–1.5 s windows).

**Alternative considered**: `cam_pp_int8` 512-d for continuity with old embedding dims — deferred pending the spike; `eres2netv2` (192-d, 71 MB FP32) — rejected: 10× larger download, never a profile default.

### Decision 5: Online path becomes polyvoice-native with zero behavioral change

**Chosen**: `SherpaEmbedder` (struct + `impl polyvoice::embedder::Embedder`) is deleted; `create_fast_channel` and `OnlineDiarizationProcessor::new` construct the polyvoice embedder directly. Nothing else in `pipeline.rs`, `recording_commands.rs`, or the frontend changes. This also resolves the pre-existing mismatch where the online-speaker-diarization spec text says "ERes2NetV2Extractor" while the implementation shipped sherpa.

**Rationale**: The `Embedder` trait is the documented injection point for `StreamingPipeline`; the swap is a constructor change in one file. Label timing, per-channel buffers, and stop-time clustering are untouched.

### Decision 6: Spike tests and dead code removal

**Chosen**: Delete the sherpa spike tests (`spike_offline_diarizer_with_int8_models`, `spike_embedding_extractor_stream_api`, `spike_embedding_ready_threshold`) and the polyvoice batch/streaming/efficient spikes (L826–920) that referenced sherpa. Keep/repoint the embedding-ready probe against `resnet34_int8` to validate Fast-mode windows (Decision 4 risk). Delete `extract_segmentation_model`, old URL constants, and `diarization_model_paths` sherpa entries; `download_with_progress` (reqwest) is dropped unless reused by the registry path (it isn't — registry downloads internally).

## Risks / Trade-offs

| Risk | Mitigation |
|------|-----------|
| **ort rc.10→rc.12 API drift breaks VAD/Parakeet compile** | Exact pin `=2.0.0-rc.12` (polyvoice's tested version, pre-rc.13 restructure); migration is task 0 with `cargo build` + tests before any diarization code changes; drift surface is two files and documented (`inputs!`, `Session`, `TensorRef`, ndarray 0.17) |
| **ResNet34 short-window embedding quality in Fast mode (1.5 s windows)** | Spike test reusing the existing embedder probe pattern on `resnet34_int8` before merge; fallback `cam_pp_int8` (512-d) is a one-line constructor swap |
| **polyvoice 0.x API changes** | Already pinned `=0.17.0`; integration isolated in the two diarization modules |
| **Model download UX regresses (no byte-level progress, no resume)** | Acceptable: two small files (~8.4 MB total), per-model progress events, idempotent `ensure`; frontend toasts stay functional |
| **Signature verification failures block downloads in release builds** | polyvoice requires signed manifest entries in release; failure surfaces as `RegistryError` mapped to the existing failed-download UX; debug builds stay lenient |
| **Stale legacy models left on disk** | `check_diarization_models` deletes sherpa-era files (spec'd scenario) |
| **Offline results change vs old engine** (new embedder/clusterer pair) | Expected and acceptable — this is the point of the change; per-channel matching and ID namespaces are preserved so UI behavior is unchanged |

## Migration Plan

1. Land dependency changes + ort rc.12 migration (task 0) and verify VAD/Parakeet independently — rollback = revert Cargo.toml pins.
2. Land engine swaps in `audio/diarization.rs` + `audio/online_diarization.rs` + model management; verify with spike tests and a manual offline run on a recorded meeting.
3. Cleanup: remove sherpa spike tests, tar/bzip2, stale-model cleanup.
4. No data migration: DB schema (speaker/speaker_label columns, diarization_status) untouched; existing labels remain valid. Users must re-download models once (8.4 MB) — the settings panel drives this via the existing download flow.

## Open Questions

1. **ort rc.10→rc.12 compatibility of `vad.rs`/`parakeet_engine/model.rs`** — unresolved until the task-0 build; the exact API drift (if any) determines the size of the migration. rc.12 is pre-rc.13-restructure, so full rewrite is not expected.
2. **Embedder choice** — `resnet34_int8` default pending the short-window spike; `cam_pp_int8` (512-d) as fallback.
3. **Progress granularity** — coarse per-model events accepted; revisit with a byte-level callback only if UX feedback demands it.
