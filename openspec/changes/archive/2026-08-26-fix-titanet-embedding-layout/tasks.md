## 1. Investigate and confirm the layout mismatch

- [x] 1.1 Dump `titanet_large.onnx` input metadata for the bundled artifact (`frontend/src-tauri/models/titanet_large.onnx`) via `ort`/polyvoice session inputs — record rank, dims, and whether expected is `[B,80,T]` or `[B,1,80,T]`; check against `frontend/src-tauri/target/debug/deps` built artifact location and bundled resource path.
- [x] 1.2 Reproduce the failure with the current code: trigger offline diarization on `meeting-3f168174-e104-41cc-9e49-9f0acbb64f00` (or a 10s stereo fixture) and capture the `Got invalid dimensions index:1 Got:<T> Expected:80` logs; confirm `segmentation=ok, embedding=0 segments`.
- [x] 1.3 Verify WeSpeaker control: embed a 1s 300 Hz sine with `polyvoice::fbank_onnx::FbankOnnxExtractor [B,T,80]` and confirm 256-d path still works when fed a WeSpeaker model (proves the generic extractor is not globally broken).

## 2. TitaNet layout adapter

- [x] 2.1 Add `TitanetAdapter` (or `TitanetLayout` helper) in `frontend/src-tauri/src/audio/embedder.rs`: wraps `polyvoice::features::FbankExtractor` + `apply_cmvn`, constructs `InferenceTensor::f32(vec![1,80,T], flat_transposed)` for `titanet_large`; preserve zero-pad and L2-normalize; keep `input_dim()=192`, `model_tag()=titanet_large`, thresholds `0.52/0.68`.
- [x] 2.2 Wire metadata-driven rank sniff at construction (rank 3→`[1,80,T]`, rank 4→`[1,1,80,T]`), log selected layout, default to `[1,80,T]` for the Recogment artifact.
- [x] 2.3 Ensure batch path is order-preserving: `embed_batch` for TitaNet uses transposed flatten per item, fans out via `parallel_embed_batch`-style pooling or `pool.checkout()` per item, and validates output `len==192` per vector.
- [x] 2.4 Keep non-TitaNet passthrough: if adapter is instantiated for a non-titanet model, it SHALL use `[1,T,80]` (no transpose) — covered by test `2.7`.
- [x] 2.5 Handle rank-4 variant if metadata shows 4-D: add optional channel dim handling behind the same sniff.
- [x] 2.6 Unit test: 1s sine → 192-d unit-norm embedding from `TitanetAdapter` against the bundled `titanet_large.onnx` (gated on file existence in CI); plus short audio 5 ms still zero-pads and returns 192-d.
- [x] 2.7 Unit test: layout isolation — synthetic fbank `[[a0..a79],[b0..b79]]` (T=2) produces flat that is transpose-correct (`[80,T]` column-major), and `[B,T,80]` path remains unchanged for a mock WeSpeaker dim.

## 3. Wire adapter into diarization pipelines

- [x] 3.1 Replace `TitanetEmbedder { inner: FbankOnnxExtractor }` construction with `TitanetAdapter::new(model_path, 192, pool_size, Cpu)` in `frontend/src-tauri/src/audio/embedder.rs:create_speaker_embedder` and `create_speaker_embedder_for_app`.
- [x] 3.2 Replace `DiarizationEmbedder = FbankOnnxExtractor` in `frontend/src-tauri/src/audio/online_diarization.rs` (`create_enhanced_embedder`) with `TitanetAdapter`; ensure Efficient and Fast stop-time buffering both call the layout-correct embedder.
- [x] 3.3 Keep `frontend/src-tauri/src/audio/diarization.rs:create_polyvoice_diarizer` and `create_polyvoice_diarizer_for_app` wiring `segmenter_pool_size` / `embedder_pool_size = min(8, ceil(0.75*cores))` unchanged.

## 4. Harden zero-embedding failure handling

- [x] 4.1 In `frontend/src-tauri/src/audio/diarization.rs:run_chunked_polyvoice_diarization` and `run_channel_diarization_stream`, after embedding accumulation, if segments existed but `all_embeddings.is_empty()` (or `valid_count==0` in `embed_segments`), return `Err` with preserved ONNX detail (`audio_signal layout`) instead of `Ok(empty)`; top-level maps to `diarization_status=failed` + `diarization-progress status=failed`.
- [x] 4.2 In `embed_segments`, distinguish layout/shape errors (`Got invalid dimensions index:1 Expected:80`) from transient errors in logs — `warn!` already present; ensure batch fallback error is not swallowed.
- [x] 4.3 Online stop-time: if a channel's per-chunk buffering had chunks but zero valid embeddings, log `WARN` with underlying detail, skip that channel's clustering, still process the other channel; if both channels empty, let offline fallback remain available.

## 5. Validation (no code edits to specs)

- [x] 5.1 Offline regression: re-run diarization on the previously failing meeting (`meeting-3f168174-e104-41cc-9e49-9f0acbb64f00`) and on a short mono fixture — assert no `Got invalid dimensions` warnings, `segments>0, speakers>=1`, `meeting_speakers.centroid` 192-d `titanet_large`, logs show `embedding` time >0.
- [x] 5.2 Stereo fixture: verify `MIC_SPEAKER_xx` vs `SPEAKER_xx` namespaces still distinct and `compute_speaker_matches` assigns per-channel correctly after layout fix.
- [x] 5.3 Online smoke: Efficient and Fast recordings (2 mic + 2 system chunks) buffer 192-d embeddings through the adapter, and stop-time grouping produces channel-isolated centroids; confirm `apply_block_speaker_to_cluster` still channel-isolated.
- [x] 5.4 Failure surfacing: synthetic all-fail case (e.g., feed `NaN` PCM) produces `status=failed` with detail, not `segments=0` success.
- [x] 5.5 Threshold sanity: with fixed layout, a two-speaker fixture still separates into 2 clusters at `0.52`; a known voiceprint still matches at `0.68`.

## 6. Cleanup and docs

- [x] 6.1 Remove or deprecate the old `TitanetEmbedder { inner: FbankOnnxExtractor }` struct if replaced (keep alias for `git grep` compatibility if needed); update `audio/mod.rs` exports.
- [x] 6.2 Update any stale comments that reference `ResNet34Adapter` in `online_diarization.rs` header (Efficient already switched to TitaNet) to reference `TitanetAdapter` and the `[B,80,T]` layout.
- [x] 6.3 Record bundled model provenance in `openspec/changes/fix-titanet-embedding-layout/proposal.md` if the fix later requires a re-export — no artifact change in this change.
