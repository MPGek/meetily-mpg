## Context

Measured 2026-09-02 via the eval harness: offline clustering over-produces speakers (voxconverse mean 23.0 hyp vs 6.5 ref; one ru-youtube podcast → 184 clusters). Code state: `TITANET_CLUSTER_THRESHOLD: f32 = 0.52` (embedder.rs:29) is passed as the AHC minimum-cosine-similarity merge criterion; `max_clusters` comes from `max_speakers.unwrap_or(0)` and polyvoice's AHC treats 0 as **no ceiling** (`AscStop::Off`). `DiarizationConfig` already threads through `create_polyvoice_diarizer` → `AhcClusterer::with_threshold`, so the plumbing skeleton exists. The harness (`diarize-eval` bin) shares the same core; the eval runner orchestrates per-file runs and pyannote DER scoring. VoxConverse ships a separate `dev` split (already inside the downloaded repo zip for RTTMs; audio zip exists upstream) usable as tuning data while `test` stays held out.

## Goals / Non-Goals

**Goals:** runtime-tunable clustering params (threshold, ceiling, gap-merge) app+harness; always-on ceiling; a sweep that can pick defaults with honest tune/validate separation; re-baselined subset gate.
**Non-goals:** no `pipeline_v2` adoption (follow-up change `adopt-polyvoice-diarization-pipeline-v2`), no model changes, no online-path retuning, no `TITANET_RECOGNITION_THRESHOLD` changes, no frontend UI work beyond what the existing settings store mechanism provides.

## Decisions

### D1. Parameter surface = extend `DiarizationConfig`
Add `cluster_threshold: f32`, `cluster_ceiling: usize`, `gap_merge_secs: f32` to the existing `DiarizationConfig` (it already carries the other knobs and reaches both app and harness). Resolution order: explicit override (harness CLI) → persisted app setting (app only) → built-in default. Alternative considered: separate `ClusteringParams` struct — rejected, fragments the existing config flow.
**Initial built-in defaults:** threshold 0.45 (polyvoice's own `DEFAULT_AHC_THRESHOLD`, first sweep center — current 0.52 is the suspected culprit), ceiling 20 (polyvoice `PipelineConfig` default for `max_speakers`; 184-speaker pathology becomes ≤20 immediately), gap_merge 0.0 (off until swept). These are starting values; final values come from D5.

### D2. Settings persistence = existing settings-store mechanism, no new UI
The three params become keys in the same persisted settings store the app already uses for diarization-adjacent settings, read where `DiarizationConfig` is constructed. A frontend Advanced-settings widget is deliberately deferred — power users can set keys via the existing settings commands; UI is a separate decision after defaults are tuned. Alternative: ship UI now — rejected as scope creep (UI copy churns once defaults change anyway).

### D3. Gap-merge implemented app-side, post-clustering
A small pass over each channel's final segment list: merge consecutive same-speaker segments when gap ≤ `gap_merge_secs`; never merge across a different-speaker segment; overlap regions untouched. Lives next to segment post-processing in `diarization.rs` so both app and harness get it for free. Alternative: polyvoice `max_gap_secs` via pipeline_v2 — rejected here (would drag in v2 adoption, which is the follow-up change).

### D4. Harness overrides via CLI flags, defaults from the same constants
`diarize-eval` gains `--cluster-threshold <f>`, `--max-clusters <n>`, `--gap-merge <secs>` building a `DiarizationConfig` with the same built-in defaults as the app (single source of truth in Rust; the bin never reads user settings — it measures defaults + explicit flags). Parity check: no flags ⇒ byte-identical to app path (existing `parity_stream_vs_in_memory_core` test extended to also assert flag-free run equals `DiarizationConfig::default()`).

### D5. Sweep = new `sweep` subcommand in eval/ + `voxconverse-dev` tuning manifest
- New manifest `voxconverse-dev.yml` (open download: dev audio zip + dev RTTMs from the repo zip already cached) marked `tuning: true`; `ru-synthetic` stays Conf-only tuning; `ru-youtube` joins the tuning pool.
- `uv run --project eval sweep --dataset X --grid cluster_threshold=0.35,0.40,0.45,0.50,0.55 --grid cluster_ceiling=12,20 --run-prefix sweep` drives the existing runner with per-candidate extra CLI args to the harness, scores each candidate run, and writes `eval/reports/sweep-<date>-<gitrev>.md` with per-candidate FA/Miss/Conf/DER rows, tuning vs validation columns kept separate.
- **Selection rule:** candidate minimizes held-out validation DER (voxconverse-test + msdwild, evaluated once for the top-2 candidates only) subject to no tuning-set component regressing >2pp from its best. Published-baseline sets are never used to select values.
Alternative: tune directly on voxconverse-test — rejected (overfits the benchmark we report against).

### D6. Re-baseline the gate in the same change
After defaults land: rerun `subset`, record new expected ranges in `eval/README.md` + manifest comments, and bump the gate assertion ranges so CI-less regression detection stays meaningful.

## Risks / Trade-offs

- [Single global threshold across domains (EN meetings vs RU YouTube)] → ceiling+gap-merge are domain-neutral; per-domain profiles (polyvoice `DomainProfile`) explicitly deferred to the v2 change; settings override exists for experiments.
- [Ceiling 20 under-clusters genuinely large meetings] → user `max_speakers` still wins when smaller; ceiling swept as a grid axis; 20 matches polyvoice's own default.
- [Gap-merge smooths away boundary jitter that DER would otherwise penalize, flattering future runs] → sweep reports gap-merge on/off as its own axis; README documents the value so deltas stay interpretable.
- [Tuned defaults change shipping behavior; re-diarizing old meetings yields different labels] → accepted (proposal); release note in eval README; rollback = store overrides back to old values (0.52 / no-op gap-merge) without code revert, since the ceiling is the only non-reversible-by-config part and can be raised.
- [Sweep compute: 5 thresholds × 2 ceilings × 2 gap values ≈ 20 candidates] → run on the small tuning pools only (voxconverse-dev subset + ru-youtube 6 files + ru-synthetic Conf), full-set scoring reserved for top-2.

## Migration Plan

Land code (defaults: 0.45/20/0.0) → run sweep → set tuned defaults + re-baseline gate in the same change before it ships. No DB migration; stored settings absent ⇒ defaults. Rollback: revert the two default constants (ceiling behavior remains, which is the safety win).

## Open Questions

- Final ceiling default if the sweep prefers 12 vs 20 (decide with data; both are spec-compliant).
- Whether `voxconverse-dev` should also feed the subset gate (currently gate composition stays spec-mandated: ru-synthetic + voxconverse-test files).
