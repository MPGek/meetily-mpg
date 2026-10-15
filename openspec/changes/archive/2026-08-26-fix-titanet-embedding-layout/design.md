## Context

See `proposal.md` — Why.

Current offline path (`audio/diarization.rs:644-836` + `audio/embedder.rs:86-147`) and online path (`audio/online_diarization.rs:76-120`) both wrap `polyvoice::fbank_onnx::FbankOnnxExtractor::new(path, 192, pool, Cpu)` as the TitaNet embedder. `FbankOnnxExtractor` computes 80-bin log-mel (default `FbankConfig`: 16 kHz, n_fft 512, win 400, hop 160) and builds `InferenceTensor [1, T, 80]` then calls `RuntimeSession::run_ordered`. That matches WeSpeaker (`ResNet34Adapter`, `ERes2NetV2Extractor`) but not the bundled `Recogment/titanet-large-onnx` graph, which validates `audio_signal` dim1==80 (mel as channel) and fails with `Got:<T> Expected:80` for every segment length (see logs: Got 1..1327). Segmentation (`segmentation-3.0`) succeeds, so the failure is isolated to embedding layout. Batch fallback falls through to per-segment, both share the same tensor construction.

Constraints: `polyvoice =0.17.0`, `ort =2.0.0-rc.12` pinned; no new runtime download; 192-d, `titanet_large`, thresholds `0.52/0.68` already shipped; chunked/streaming pipeline and fixed pool `min(8, ceil(0.75*cores))` must stay.

## Goals / Non-Goals

**Goals:**

- Make the bundled `titanet_large.onnx` produce valid 192-d embeddings for any T >= 1 frame (short pads stay valid) on both offline and online paths.
- Preserve order-preserving batch semantics and existing clustering/recognition pipeline.
- Fail loudly when zero valid embeddings remain, instead of `segments=0` silent success.

**Non-Goals:**

- Changing segmentation model, clustering (`AhcClusterer` + `MinClusterSizeClusterer`), chunking/overlap, or thresholds.
- Reintroducing legacy `resnet34_int8` fallback or runtime download.
- Re-exporting the ONNX or changing the model artifact in this change (handled as alternative if layout fix is insufficient — see Open Question).
- Frontend redesign; only error propagation.

## Decisions

### Decision 1 — Introduce a TitaNet-specific layout adapter (not a generic fbank change)

**Choice:** Keep `FbankOnnxExtractor` unchanged for WeSpeaker compatibility. Add a thin wrapper `TitanetAdapter` (or `titanet_layout` helper) in `audio/embedder.rs` that reuses `FbankExtractor` + `apply_cmvn` but constructs the ONNX tensor as `[1, 80, T]` (transpose of the flattened `[T,80]` row-major buffer) when `model == titanet_large`. `create_speaker_embedder` and `online_diarization::create_enhanced_embedder` instantiate the wrapper, not the generic extractor directly.

**Rationale:** `polyvoice::fbank_onnx` is shared by multiple adapters that assume `[B,T,80]`; mutating it would risk ResNet/CAM++ regressions and couples meetily to an upstream layout that TitaNet does not use. A wrapper isolates the fix, keeps `TitanetEmbedder` 192-d, and lets `test_non_titanet_paths_unchanged` assert `[B,T,80]` still works.

**Alternatives considered:**

- *Mutate `FbankOnnxExtractor` to transpose internally* — rejected (upstream crate change, broader blast radius).
- *Re-export TitaNet ONNX to accept `[B,T,80]`* — viable but requires NeMo export pipeline, Python, model re-signing; deferred unless layout fix does not match the actual graph (see Open Question).
- *Add a `use_titanet_layout: bool` flag to the generic path* — equivalent to wrapper but less explicit; wrapper is self-documenting.

### Decision 2 — Transpose at the tensor construction seam, with length-aware zero pad

**Choice:** In `TitanetAdapter::embed`/`embed_batch`, call `FbankExtractor::extract` → `apply_cmvn` → `n_frames × 80` matrix, then produce `flat` in column-major `80 × T` order (or keep row-major and build `[1,80,T]` with `flat` filled transposed), then `InferenceTensor::f32(vec![1, 80, T], flat)`. Short audio (< win_length) stays zero-padded as `FbankOnnxExtractor` already does. `T==1` edge still yields `[1,80,1]`.

**Rationale:** Minimal copy, no FFT recompute. Matches observed ONNX expectation (dim1==80 constant). Matches typical NeMo TitaNet preprocessing (80 mels as channel, time last). Keeps CMVN per-bin-mean logic identical.

**Alternatives considered:**

- *Insert an extra channel dim `[1,1,80,T]`* — easy to add if metadata dump shows 4-D input; adapter can branch on `session.inputs[0].shape` rank at construction time.

### Decision 3 — Resolve layout from model metadata at construction when possible

**Choice:** At `TitanetAdapter::new`, query `ort_session::inputs` (via `RuntimeSession::from_path` metadata) for `audio_signal` rank/shape. If rank==3 and dim1==Some(80) or symbolic, assume `[B,80,T]`; if rank==2 `[B,T]` treat as waveform (not expected); log `info!("TitaNet input layout: {:?}", shape)`. Default to `[1,80,T]` for `titanet_large`.

**Rationale:** Makes the fix self-checking against the actual bundled artifact and protects against a future mirror that ships a `[B,T,80]` export — would auto-select correctly.

### Decision 4 — Hardening: zero-valid-embedding → failure, not empty success

**Choice:** In `audio/diarization.rs:run_chunked_polyvoice_diarization` + `run_channel_diarization_stream`, after embedding, if `all_embeddings.is_empty()` but `all_segments` had items or at least one raw segment existed, return `Err("Embedding produced zero valid vectors: {underlying detail}")` instead of `Ok(empty)`. Top-level `run_diarization_blocking_with_app` maps to `diarization_status=failed` + progress `failed`. Partial success (some valid) still clusters the valid subset.

**Rationale:** The current `all_segments.is_empty() → Ok(empty)` path masked the bug as success. The fix restores the existing `Diarization failure` spec requirement and gives actionable logs.

**Alternatives considered:** *Thresholded success (e.g. <2 embeddings → fail)* — rejected; clustering already handles <2 via `MinClusterSizeClusterer`.

### Decision 5 — Apply fix to both offline chunked and online per-chunk paths

**Choice:** Both `audio/embedder.rs` (offline `TitanetEmbedder`) and `audio/online_diarization.rs` (`DiarizationEmbedder`) go through the same `TitanetAdapter::new`/`embed` path. `polyvoice::SpeakerDiarization` streaming pipeline's own embedder is not replaced (pinned crate) — keep existing Fast `StreamingPipeline` but ensure the parallel per-chunk buffering for recognition uses the corrected adapter.

**Rationale:** Logs show offline failure; online Efficient/Fast buffering reuses `FbankOnnxExtractor` directly, so it would replay the bug at stop-time. Fixing at the adapter factory covers both.

## Risks / Trade-offs

- **Layout still wrong after transpose (e.g., 4-D `[1,1,80,T]` or 64 mels)** → Mitigation: metadata-driven rank check + toggle; keep a small feature flag/env `MEETILY_TITANET_LAYOUT=[B80T|BT80|B1-80-T]` for rapid fallback without rebuild; validate with a 1-second sine test that reaches 192-d in CI.
- **Extra transpose copy adds latency** → Mitigation: copy is O(T*80) per segment (~10k floats per 1s), negligible vs ONNX inference; batch transpose reuses same buffer.
- **Short segments (< win_length) after transpose still fail orthogonally (audio too short vs empty fbank)** → Mitigation: preserve existing zero-pad + empty-fbank error; those segments are correctly skipped, not layout errors.
- **Upstream `polyvoice` later adds native TitaNet adapter** → Mitigation: wrapper is thin; swap to upstream adapter when available with same interface.
- **Masking of original 256-d voiceprints** (enhanced-only already done) — no additional migration; existing 192-d caches invalidated by prior shape bug remain but will be overwritten on next successful run.

## Migration Plan

1. Land adapter + hardening, no DB migration.
2. On next diarization run, successful embeddings overwrite prior empty caches; no manual cleanup.
3. Rollback: revert `audio/embedder.rs` + `online_diarization.rs` to `FbankOnnxExtractor` direct; diarization reverts to failing layout — safe, no data corruption.

## Open Questions

- Exact rank of `titanet_large.onnx` input: is it `[B,80,T]` or `[B,1,80,T]`? Inspect with `read_model_metadata_props` / `ort` session input shape dump in a one-off dev run; if 4-D, extend adapter to `vec![1,1,80,T]`. Tracked as task T1.
- Does `Recogment/titanet-large-onnx` apply its own normalization (beyond `apply_cmvn`) that affects cosine threshold calibration? If post-fix thresholds drift, recalibrate on held-out data (out of scope for this fix; current `0.52/0.68` stand).
