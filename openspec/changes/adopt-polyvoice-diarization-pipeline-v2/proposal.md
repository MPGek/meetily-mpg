## Why

Even after clustering-parameter tuning (tune-diarization-clustering-params), the offline pipeline remains architecturally behind the pyannote 3.1 baselines we score against (measured 2026-09-02: VoxConverse DER 33.6 vs 11.3, MSDWild 41.5 vs 25.3, with Conf dominating): the app embeds each segment once (sparse, median 1.4 s fragments), decodes powerset posteriors without hysteresis (misses low-confidence speech), assigns one speaker per frame in overlap regions, and has no speaker-count model. polyvoice 0.17.0 — already a dependency — ships `pipeline_v2`, a Rust port of exactly that pyannote 3.1 architecture (resegmentation, dense embedding windows, calibrated binarization, Hungarian local→global mapping, two-speaker overlap assignment, NME-SC/VBx automatic count, AS-Norm/PLDA). The gap is closable by adopting vendored, tested code rather than writing new algorithms.

## What Changes

- **BREAKING** Offline diarization (`diarization.rs` chunked core) is reworked to call polyvoice `pipeline_v2` (or its components): resegmentation windows + `embed_window_secs` dense embeddings, `BinarizationConfig` hysteresis, overlap-aware two-speaker output, and automatic speaker-count selection (`ClustererKind::Vbx` or `NmeSc`) replacing fixed-threshold AHC — behind the tunable parameter surface introduced by the prerequisite change.
- The `diarize-eval` harness binary and the app keep using the identical core (existing parity requirement); the parity integration test is re-run and re-accepted on the new architecture.
- Ship VBx PLDA parameters as a new model asset (polyvoice model-registry download or bundled resource), with the same 3-location resolution and integrity checks as existing diarization models.
- Re-baseline the eval: subset gate thresholds and the README baseline table are updated to post-adoption numbers; the 2.3-style app-vs-harness parity check is repeated on a stored meeting.
- Performance budget: dense embedding multiplies TitaNet calls; offline diarization wall-time must stay within a documented factor (measured before/after on voxconverse) and long-recordings memory behavior is preserved.

Non-goals: no online/streaming diarization changes (the streaming pipeline is a separate follow-up), no model replacements (segmentation-3.0 + TitaNet stay), no voiceprint recognition-threshold changes, no per-domain calibration profiles beyond what `pipeline_v2` already supports.

## Capabilities

### New Capabilities

_(none — this is a behavioral upgrade of the existing offline pipeline)_

### Modified Capabilities

- `speaker-diarization`: the "Speaker diarization pipeline" requirement now mandates resegmentation-based dense embedding, calibrated binarization, overlap-aware multi-speaker output, and automatic speaker-count selection for offline diarization; "Long recordings process without unbounded memory growth" must hold under the new architecture; "Online diarization uses the same engine family" is preserved (engine family unchanged; streaming path untouched).

## Impact

- `frontend/src-tauri/src/audio/diarization.rs` — core rework: `PolyvoiceDiarizer`/`run_chunked_polyvoice_diarization` delegate to `polyvoice::pipeline_v2` (builder + `PipelineConfig`); `ClusteredEmbedding`/`DiarizationSegment` output shapes preserved so `meeting_speakers`, `speaker_embeddings` provenance, voiceprint enrollment and live-label persistence keep working.
- `frontend/src-tauri/src/audio/embedder.rs` / `segmentation.rs` — adapters may be superseded by v2 internals; `TITANET_CLUSTER_THRESHOLD` remains the fallback knob for the AHC kind.
- `src/bin/diarize_eval.rs` — inherits the new core automatically (parity), plus PLDA asset resolution.
- Model assets & build: PLDA parameter files added to the bundled model set (download script, `tauri.conf.json` resources, integrity checks); packaging size delta.
- `eval/`: re-baselined subset gate, updated report/README numbers, extended parity test.
- Dependencies: polyvoice `vbx` feature (and its `ort` requirements) enabled in `Cargo.toml`; ONNX Runtime CUDA still not required (CPU EP parity with current shipping behavior).
- Users: re-diarizing an existing meeting after this change will produce different (expected better) speaker blocks and turn boundaries.
