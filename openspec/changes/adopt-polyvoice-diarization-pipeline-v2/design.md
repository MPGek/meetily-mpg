# Design — adopt polyvoice pipeline_v2 for offline diarization

## Context

See proposal.md for motivation. Current state (post tune-diarization-clustering-params, commit 9b4b79e):

- `diarization.rs` runs a chunked core: ffmpeg streaming decode → per-chunk segmentation (`segmentation.rs` adapter) → one embedding per raw segment (`embedder.rs`, batched TitaNet) → pooled embeddings → global AHC with runtime `cluster_threshold` (0.60), always-enforced `cluster_ceiling` (128), post-clustering `gap_merge_secs` (0.3) → `ClusteredEmbedding` / `DiarizationSegment` → transcript matching, `meeting_speakers` + `speaker_embeddings` caches, voiceprint enrollment/recognition.
- polyvoice 0.17.0 `pipeline_v2` is vendored but unused: `Pipeline::run(&self, samples: &[f32], sr)` is monolithic over the whole decoded buffer; components (`segmentation` with `BinarizationConfig`, `resegmentation`, `clusterer` incl. `ClustererKind::{Ahc{threshold}, NmeSc, Vbx}`, `hungarian`, `overlap`) are public modules; the builder accepts injected `Segmenter`/`Embedder`/`Clusterer`/`Resegmenter`.
- Constraints from existing specs: bounded memory for long recordings (chunked decode, per-channel 16kHz f32 ≈ 230 MB/hour), enhanced-model 3-location resolver with no runtime download, app/harness core parity, online (streaming) diarization untouched.

## Goals / Non-Goals

**Goals:**
- Offline accuracy: close the Conf-dominated gap (voxconverse Conf 21.45, msdwild Conf 28.84); ship gate: **msdwild Conf < 28.27** on the full-set run.
- Keep every downstream consumer (transcript matching, cluster caches, voiceprint enrollment, live-label persistence) working unchanged in shape: `ClusteredEmbedding`/`DiarizationSegment` remain the core's output contract.
- Preserve bounded-memory chunked processing and multi-core batched embedding.
- No-rebuild rollback to the pre-adoption clustering semantics via settings.

**Non-Goals:**
- No online/streaming changes; no model swaps (segmentation-3.0 + TitaNet-Large stay); no new per-domain calibration beyond what v2 config exposes; no settings UI polish beyond a clusterer-kind selector.

## Decisions

### D1. Drive v2 components inside the existing chunk loop — not the monolithic `Pipeline::run()`

`pipeline_v2::run()` takes the entire decoded channel as one `&[f32]` (≈230 MB/hour/channel), violating the long-recordings memory spec. Instead the reworked `run_chunked_polyvoice_diarization` keeps its ffmpeg-streaming chunk loop and calls v2 stages directly:

```
per chunk (bounded audio buffer):
  segmenter.segment(chunk)  -> frame posteriors + raw segments   [binarization on]
  accumulate posteriors (stitched across the 5s chunk overlap)
  dense windows: embed_window_secs windows w/ w/2 hop per segment
                 -> embed_batch -> accumulate 192-d embeddings
after all chunks (global stage):
  resegmentation over stitched posteriors -> speaker turns (+ overlap regions)
  clusterer (Vbx | NmeSc | Ahc) over pooled embeddings -> global labels
  Hungarian local->global mapping; two-speaker assignment in overlap regions
  gap-fill (max_gap_secs)
```

Alternatives considered: (b) buffer the whole channel and call `run()` — rejected, breaks the memory spec; (c) run full v2 per chunk then merge labels — rejected, resegmentation and clustering are global stages and per-chunk global passes break cross-chunk label consistency.

**Escape hatch (spike-gated):** if posterior stitching or resegmentation composition proves impractical against the vendored API, degrade to adopting only the clustering + overlap-assignment stages over the current per-segment embeddings (still replaces AHC with VBx/NME-SC and adds overlap output), and record the reduced scope in the specs before implementation continues. Task 1 decides this.

### D2. Clusterer: `NmeSc` default (revised — spike finding 2026-09-07), kind is a setting, `Ahc` is the rollback

**Original decision** was `ClustererKind::Vbx` (VBx HMM + PLDA, automatic count) as default. The spike invalidated it: the vendored PLDA parameter set is dimension-locked to 256-d WeSpeaker ResNet34 embeddings (`plda_mean1.npy` shape `(256,)`, `plda_lda.npy` `(256,128)` — verified from the manifest-pinned fixtures), and polyvoice's own v2 profile path pairs VBx with `ResNet34Adapter`. The app's enhanced TitaNet-Large is 192-d; `PldaModel::transform` would panic on the broadcast mismatch (no dim check in `VbxClusterer`). The change's non-goal "no model replacements (segmentation-3.0 + TitaNet stay)" forbids the 256-d swap, and training 192-d PLDA params is new-algorithm work the proposal explicitly rejected.

**Revised decision**: `ClustererKind::NmeSc` (spectral normalized-maximum-eigengap, automatic count, cosine-affinity → dimension-agnostic, no asset) is the built-in default. New persisted setting `diarizationClusterer` ∈ `vbx|nmesc|ahc`, default `nmesc`. `vbx` remains parseable (forward-compatible if a PLDA-compatible 256-d family ever ships) but the clusterer factory SHALL return a clear actionable error for the enhanced family — no panic, no silent kind switch (consistent with "no silent fallback model set"). **No PLDA files are bundled**; the model-management requirement and the asset tasks are amended accordingly. `Ahc` remains the no-rebuild rollback. The acceptance gates (msdwild Conf < 28.27, no voxconverse/ru-youtube regression) are measured with the `nmesc` default; if `nmesc` misses them, the 6.2 sweep falls back to `ahc` re-tune, not to VBx.

### D3. Parameter mapping onto the existing tuning surface

| setting | v2 mapping | notes |
| --- | --- | --- |
| `diarizationClusterer` (new) | `ClustererKind` | default `nmesc` (revised D2); `vbx` gated to an actionable error |
| `diarizationClusterThreshold` | `Ahc { threshold }` only | ignored (no-op) for vbx/nmesc; UI text updated |
| `diarizationClusterCeiling` | `PipelineConfig::max_speakers` | u8: clamp stored values >255 to 255, log; default 128 fits |
| `diarizationGapMergeSecs` | `max_gap_secs` (pipeline gap-fill) | replaces the app's post-clustering merge pass; 0 disables |

The post-clustering gap-merge pass in `diarization.rs` is deleted once `max_gap_secs` covers it (same observable rule: consecutive same-speaker segments bridged, cross-speaker boundaries and overlaps untouched).

### D4. Dense windows + binarization are built-in constants, not settings

`embed_window_secs = 5.0` (w/2 hop, per v2 docs) is a compiled-in value, exposed to the harness sweep via CLI overrides only (extend the existing `--harness-arg` surface). Binarization: the vendored `BinarizationConfig` defaults are `0.5/0.5/0/0` — plain thresholding with no hysteresis — so the spike selects the shipped hysteresis constants by probing on voxconverse-dev (they are compiled-in values, harness-sweepable via `--binarization`, not persisted settings; rationale: the sweep protocol exists to choose defaults later; adding three more persisted settings now is speculative configurability).

### D5. Output shapes preserved; overlap is representable end-to-end

- `DiarizationSegment` may now contain temporally overlapping segments with distinct labels (mic/system channel namespacing unchanged). Transcript attribution keeps max-overlap matching; token-level attribution picks, among turns covering a token, the one with the larger covered duration; a segment spanning an overlap boundary still splits per the existing rules.
- Dense embeddings are aggregated **per source segment**: mean of its windows, L2-normalized, becomes that segment's embedding in `ClusteredEmbedding` — so the bounded exemplar cache (`speaker_embeddings`), centroids (`meeting_speakers`), enrollment, and τ=0.68/0.7 recognition all keep their current per-segment semantics. Raw dense windows feed clustering only and are not persisted.
- Singleton-cluster pruning (post-processing pass) is removed: resegmentation + `min_speech_secs` supersede it, and v2 measurements show pruning is net-negative for the powerset pipeline (collapses short clips).

### D6. Parity, eval re-baseline, performance budget

- `diarize-eval` links the same core; gains `--clusterer=<vbx|nmesc|ahc>` (+ existing threshold/ceiling/gap flags). Parity re-accepted on the stored meeting (existing 2.3-style check).
- Re-baseline: full-set runs (voxconverse, msdwild, ru-youtube, ru-synthetic) → README baseline table + `subset_gate:` ranges updated; acceptance gate msdwild Conf < 28.27; no voxconverse/ru-youtube regression past new gates.
- Wall-time budget: offline diarization ≤ **3×** pre-adoption wall time on the voxconverse subset (dense windows + overlap windows are the multiplier), measured in task 1's spike and re-measured at re-baseline; peak memory behavior unchanged (same chunked decode).

## Risks / Tradeoffs

- [Dense embedding multiplies TitaNet calls] → batched embed path + fixed session pool already in place; budget in D6; if exceeded, tune `embed_window_secs` down via harness sweep before shipping.
- [VBx unstable on very short / single-speaker channels (few embeddings)] → spike includes a single-speaker meeting; rollback kind `nmesc`/`ahc` via setting; built-in default is re-decided at re-baseline.
- [Russian-domain quality unknown — PLDA/VBx priors are English-tuned] → gates on ru-youtube/ru-synthetic (Conf-only) decide whether `vbx` or `nmesc` ships as default; kind is already a value, not a code change.
- [Component stitching (posteriors across chunk overlaps) produces boundary artifacts] → 5 s chunk overlap already guarantees turn coverage; parity test on a stored long meeting; escape hatch D1.
- [VBx PLDA params are 256-d-locked (WeSpeaker ResNet34), incompatible with the 192-d enhanced TitaNet-Large family] → **materialized in the spike**; resolved by shipping `nmesc` as the default kind and gating `vbx` to a clear actionable error (revised D2); no PLDA asset is bundled.
- [Users' stored ceiling >255 or threshold silently ignored under vbx] → clamp + log at resolve time; settings UI labels threshold "(AHC only)".
- [Re-diarized meetings get different labels/turn boundaries] → already the accepted pattern from the tuning change; release note + settings rollback.

## Migration Plan

1. Land v2 core behind default `diarizationClusterer=nmesc`; no DB migrations; caches are rewritten by the next diarization run.
2. Rollback without revert: set `diarizationClusterer=ahc` (plus stored threshold 0.52 / gap 0.0 to recover exact pre-adoption behavior, mirroring the tuning-change rollback documented in eval/README).
3. eval/README: new sweep section for kind + window params; re-baselined gate table.

## Open Questions

- ~~Exact `BinarizationConfig` onset/offset values shipped by polyvoice 0.17.0 defaults~~ **Resolved (spike):** the vendored defaults are `onset=0.5, offset=0.5, min_duration_on=0.0, min_duration_off=0.0` — plain thresholding, no hysteresis. Since the spec mandates calibrated hysteresis, the spike selects the shipped constants by probing hysteresis settings on voxconverse-dev (harness `--binarization` passthrough).
- ~~PLDA asset byte size and whether the model-registry download or a build-time bundle is the primary source~~ **Resolved (spike):** the six PLDA `.npy` files total ~265 KB (CC-BY-4.0, pyannote community-1 derived) but are 256-d-locked and unusable with the 192-d enhanced family — see revised D2; no PLDA assets are bundled.
