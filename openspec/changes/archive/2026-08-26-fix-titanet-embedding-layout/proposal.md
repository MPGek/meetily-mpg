## Why

Offline (and likely online) diarization with the bundled `titanet_large` model fails silently: segmentation succeeds but every embedding extraction errors with `Got invalid dimensions for input: audio_signal index:1 Got:<T> Expected:80`, producing 0 segments, 0 speakers, and leaving transcripts unlabeled. The enhanced-only cut (`remove-standard-diarization-models`) removed the working fallback, so the layout mismatch between `polyvoice::fbank_onnx::FbankOnnxExtractor` (`[B, T, 80]`) and the NeMo TitaNet ONNX export (`[B, 80, T]`) is now a hard product failure.

## What Changes

- Fix TitaNet embedding input layout so the 80 mel bins are in dimension 1 as the ONNX graph expects, while preserving `polyvoice` WeSpeaker behavior for any non-TitaNet path.
- Provide a TitaNet-specific adapter/layout path (transpose or model-aligned tensor construction) instead of routing `titanet_large.onnx` through the generic `[B, T, 80]` `FbankOnnxExtractor` directly.
- Harden diarization error handling so a batch that produces zero valid embeddings does not silently report `segments=0, speakers=0` success; it surfaces a clear inference/layout failure and sets `diarization_status=failed`.
- Apply the fix to both offline (`audio/diarization.rs`) and online (`audio/online_diarization.rs`) embedder construction, keeping the 192-d, `titanet_large` family, thresholds (`0.52` clustering / `0.68` recognition), and streaming/chunked pipeline otherwise unchanged.
- No model re-download flow reintroduced; no new runtime download. Bundled `segmentation-3.0 + titanet_large` remain the sole model set.

## Capabilities

### New Capabilities

<!-- none -->

### Modified Capabilities

- `speaker-diarization`: offline pipeline SHALL feed TitaNet embeddings with the layout the model expects and SHALL fail loudly (instead of silent 0-segment success) when embedding produces no valid vectors.
- `online-speaker-diarization`: Efficient/Fast online embedding paths SHALL use the same TitaNet-correct layout, so live clustering is not subject to the same failure.

## Impact

- Backend: `frontend/src-tauri/src/audio/embedder.rs` (new `TitanetAdapter` or corrected tensor construction; `FbankOnnxExtractor` transpose handling), `frontend/src-tauri/src/audio/diarization.rs` (`embed_segments` clustering guard, `create_polyvoice_diarizer` pool wiring), `frontend/src-tauri/src/audio/online_diarization.rs` (`DiarizationEmbedder` construction + per-chunk embedding), `frontend/src-tauri/src/audio/segmentation.rs` (no change).
- Build: `frontend/src-tauri/build.rs` / `models/` unchanged (same `Recogment/titanet-large-onnx` artifact, 101 MB); only inference layout changes.
- Frontend: no UI changes except diarization failure now correctly shows `failed` instead of empty success.
- Dependencies: `polyvoice =0.17.0`, `ort =2.0.0-rc.12` remain pinned; no new crate required unless a small transpose helper is added.
- DB: no migration; `speaker_embeddings.model='titanet_large'` stays 192-d.
