## Why

The diarization eval harness (add-diarization-eval-harness, 2026-09-02) measured that offline speaker diarization over-clusters severely on real data: mean 23.0 hypothesis speakers vs 6.5 reference on VoxConverse (DER 33.6 vs pyannote 3.1 baseline 11.3, Conf component 25.3 of it), and a single long podcast recorded 184 "speakers" vs 2.8 reference. Root causes are parameter-level, not architectural: `TITANET_CLUSTER_THRESHOLD = 0.52` is stricter than polyvoice's own tuned default (0.45), and with `max_speakers` unset AHC runs with **no cluster-count ceiling** (`AscStop::Off`), so fragmented short segments (median 1.4 s vs 2.8 s reference) each seed their own cluster. These constants are compile-time and untunable, so the tuning loop is impossible without code changes today.

## What Changes

- Make offline clustering parameters runtime-configurable: AHC merge threshold, cluster-count ceiling, and same-speaker gap-merge — via app settings (persisted) and matching overrides on the `diarize-eval` harness binary (CLI/env) so the harness can measure any candidate without a rebuild.
- Add a cluster-count ceiling that is always active (default derived from `max_speakers` when set, else a sane cap) — eliminates the 184-speaker pathology regardless of tuning.
- Grid-sweep the parameters using the eval harness on the real-data sets (voxconverse, msdwild, ru-youtube; ru-synthetic read Conf-only due to its broken annotation timeline), then set new tuned defaults.
- Re-baseline the `subset` regression gate against the tuned defaults and record the resulting DER floor in the eval report/README.

Non-goals: no pipeline architecture changes (resegmentation/dense embedding/VBx live in a follow-up change), no model swaps, no online-diarization changes, no voiceprint recognition threshold changes (`TITANET_RECOGNITION_THRESHOLD` stays separate and untouched).

## Capabilities

### New Capabilities

- `diarization-param-tuning`: runtime-configurable offline clustering parameters (threshold, count ceiling, gap-merge) for both the app pipeline and the `diarize-eval` harness, plus the sweep/re-baseline protocol and its acceptance gate.

### Modified Capabilities

- `speaker-diarization`: the "Speaker diarization pipeline" requirement gains behavioral constraints — clustering must respect a hard speaker-count ceiling and tunable merge parameters with new tuned defaults; existing meetings re-diarized after the default change will produce different labels (accepted).

## Impact

- `frontend/src-tauri/src/audio/embedder.rs` (`TITANET_CLUSTER_THRESHOLD` becomes a resolved setting with the current value as fallback), `audio/diarization.rs` (plumb params through `DiarizationConfig`/`create_polyvoice_diarizer`, pass a real ceiling instead of 0), `audio/online_diarization.rs` (read-only consumer check — not retuned).
- `src/bin/diarize_eval.rs` (new `--cluster-threshold` / `--max-clusters` / `--gap-merge` flags).
- Settings plumbing: Tauri settings store + frontend settings UI (advanced, optional exposure), `DiarizationConfig` extension.
- `eval/` harness: sweep driver (subset of the runner), manifests/README baseline updates; `eval/out/` re-baselined.
- Parity: harness and app must keep using the identical core path (existing spec requirement) — parameter plumbing must not fork behavior.
