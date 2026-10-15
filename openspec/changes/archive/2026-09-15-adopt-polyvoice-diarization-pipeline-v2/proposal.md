## Why

The prerequisite clustering-parameter tuning (tune-diarization-clustering-params, archived 2026-09-05) confirmed the gap is architectural, not parametric. Post-tuning measurements (report `eval/reports/2026-09-05-16c261d.md`, defaults thr 0.60 / ceil 128 / gap 0.3): VoxConverse DER 29.61 vs pyannote 3.1 baseline 11.3, MSDWild 42.08 vs 25.3 — and Confusion still dominates both (21.45 pp = 72% of DER on voxconverse, 28.84 pp = 69% on msdwild). Tuning improved voxconverse by ~4 pp but **msdwild was a null result** (Conf +0.57 pp, failed its gate); the acceptance report explicitly defers the msdwild regression to this change. The app embeds each segment once (sparse, median 1.4 s fragments), decodes powerset posteriors without hysteresis (misses low-confidence speech), assigns one speaker per frame in overlap regions, and has no speaker-count model — none of which threshold sweeps can fix. polyvoice 0.17.0 — already a dependency — ships `pipeline_v2`, a Rust port of exactly that pyannote 3.1 architecture (resegmentation, dense embedding windows, calibrated binarization, Hungarian local→global mapping, two-speaker overlap assignment, NME-SC/VBx automatic count, AS-Norm/PLDA). The gap is closable by adopting vendored, tested code rather than writing new algorithms.

## What Changes

- **BREAKING** Offline diarization (`diarization.rs` chunked core) is reworked to drive polyvoice `pipeline_v2` components (resegmentation windows + `embed_window_secs` dense embeddings, `BinarizationConfig` hysteresis, overlap-aware two-speaker output, gap-fill via `max_gap_secs`) with overlap-aware two-speaker output and dense-window embeddings replacing the sparse per-segment path — behind the tunable parameter surface introduced by the prerequisite change, extended with a clusterer-kind setting (`ClustererKind::Ahc` default selected by the 6.2 sweep, `NmeSc` selectable; `Vbx` ships gated to an actionable error because its bundled PLDA parameters require 256-d embeddings while the enhanced TitaNet-Large family is 192-d).
- The `diarize-eval` harness binary and the app keep using the identical core (existing parity requirement); the parity integration test is re-run and re-accepted on the new architecture.
- ~~Ship VBx PLDA parameters as a new model asset~~ **Spike revision:** no PLDA asset ships — the vendored parameters are 256-d-locked (pyannote community-1) and unusable with the 192-d enhanced family; `vbx` kind selection fails with an actionable error naming the constraint (no silent kind switch).
- Re-baseline the eval: subset gate thresholds and the README baseline table are updated to post-adoption numbers; the 2.3-style app-vs-harness parity check is repeated on a stored meeting.
- Acceptance target carried over from the tuning null result: **msdwild Conf < 28.27** (its pre-tuning value) on the full-set run, with no voxconverse/ru-youtube regression beyond the re-baselined gates.
- Performance budget: dense embedding multiplies TitaNet calls; offline diarization wall-time must stay within a documented factor (measured before/after on voxconverse) and long-recordings memory behavior is preserved.

Non-goals: no online/streaming diarization changes (the streaming pipeline is a separate follow-up), no model replacements (segmentation-3.0 + TitaNet stay), no voiceprint recognition-threshold changes, no per-domain calibration profiles beyond what `pipeline_v2` already supports.

## Capabilities

### New Capabilities

_(none — this is a behavioral upgrade of the existing offline pipeline)_

### Modified Capabilities

- `speaker-diarization`: the "Speaker diarization pipeline" requirement now mandates resegmentation-based dense embedding, calibrated binarization, overlap-aware multi-speaker output, and automatic speaker-count selection for offline diarization; "Long recordings process without unbounded memory growth" must hold under the new architecture; "Online diarization uses the same engine family" is preserved (engine family unchanged; streaming path untouched); singleton-cluster pruning is removed (superseded by resegmentation + min-speech filtering).
- `diarization-param-tuning`: the runtime parameter surface gains a clusterer-kind setting (`vbx`/`nmesc`/`ahc`, default `ahc` selected by the 6.2 sweep); the merge threshold applies only to the `ahc` kind; the ceiling is enforced for every kind (clamped to the pipeline's 255 maximum); the harness accepts a `--clusterer` override.

## Impact

- `frontend/src-tauri/src/audio/diarization.rs` — core rework: `PolyvoiceDiarizer`/`run_chunked_polyvoice_diarization` delegate to `polyvoice::pipeline_v2` (builder + `PipelineConfig`); `ClusteredEmbedding`/`DiarizationSegment` output shapes preserved so `meeting_speakers`, `speaker_embeddings` provenance, voiceprint enrollment and live-label persistence keep working.
- `frontend/src-tauri/src/audio/embedder.rs` / `segmentation.rs` — adapters may be superseded by v2 internals; `TITANET_CLUSTER_THRESHOLD` remains the fallback knob for the AHC kind.
- `src/bin/diarize_eval.rs` — inherits the new core automatically (parity), plus PLDA asset resolution.
- Model assets & build: ~~PLDA parameter files added to the bundled model set~~ spike revision: no new model asset ships (VBx gated — see above); no packaging size delta.
- `eval/`: re-baselined subset gate, updated report/README numbers, extended parity test.
- Dependencies: polyvoice `vbx` + `resegmentation` + `spectral` features enabled in `Cargo.toml` (the latter two required by the v2 stage components and the selectable `nmesc` kind; `vbx` keeps the kind parseable for the gated error path); ONNX Runtime CUDA still not required (CPU EP parity with current shipping behavior).
- Users: re-diarizing an existing meeting after this change will produce different (expected better) speaker blocks and turn boundaries.
