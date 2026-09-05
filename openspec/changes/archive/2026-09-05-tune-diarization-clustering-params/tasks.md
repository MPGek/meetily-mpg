## 1. Rust parameter surface

- [x] 1.1 Extend `DiarizationConfig` with `cluster_threshold` (default 0.45), `cluster_ceiling` (default 20), `gap_merge_secs` (default 0.0); pass threshold+ceiling into `AhcClusterer::with_threshold` so no code path can run `AscStop::Off`; update the default-asserting tests in `embedder.rs`; verify `cargo check -p meetily` passes and `cargo test -p meetily diarization` passes
- [x] 1.2 Implement post-clustering same-speaker gap-merge over each channel's final segments (never cross a different-speaker segment; overlap regions untouched); verify with new unit tests: gap ≤ window bridged, cross-speaker boundary preserved, `gap_merge_secs = 0` is a no-op
- [x] 1.3 Resolve the three parameters from persisted app settings with built-in-default fallback at `DiarizationConfig` construction; verify a unit test on the resolver (stored override wins, unset falls back) and a manual check that a stored threshold override changes speaker-block count on one stored meeting
- [x] 1.4 Add `--cluster-threshold`, `--max-clusters`, `--gap-merge` flags to the `diarize-eval` bin wiring into `DiarizationConfig`; rebuild release bin; verify flag-free run is byte-identical to the app-path default on the parity fixture (extend `parity_stream_vs_in_memory_core` with a flag-free vs `DiarizationConfig::default()` assertion) and one changed-threshold run differs only in clustering labels/merges

## 2. Eval: tuning data and sweep driver

- [x] 2.1 Add `eval/manifests/voxconverse-dev.yml` (`tuning: true`, dev audio zip + `voxconverse-master/dev/*.rttm` from the cached repo zip) and verify `download` + `normalize` produce the canonical layout with counts matching and RTTM/UEM parse checks passing
- [x] 2.2 Add per-candidate harness argument passthrough to the eval runner (`--harness-arg` repeatable, appended to each `diarize-eval` invocation) and run-ids for sweep output isolation; verify a run with `--harness-arg --cluster-threshold=0.35` produces a different hypothesis set than default under a distinct run-id
- [x] 2.3 Implement `sweep` subcommand: parse `--grid param=v1,v2,...` axes, run the cross-product over tuning datasets into per-candidate run-ids, score each, write `eval/reports/sweep-<date>-<gitrev>.md` with per-candidate FA/Miss/Conf/DER rows and separate tuning vs held-out columns; verify a 2×1 grid on ru-youtube completes end-to-end and the report has one row per candidate

## 3. Sweep and defaults

- [x] 3.1 Run the full grid (threshold {0.35,0.40,0.45,0.50,0.55} × ceiling {12,20} × gap-merge {0.0,0.3}) over the tuning pools (voxconverse-dev, ru-youtube, ru-synthetic Conf-only); verify `sweep-*.md` contains all 20 candidates (5×2×2; "30" in the original text was a miscount — design.md says ≈20) with complete components
- [x] 3.2 Select the top-2 candidates by tuning metrics and evaluate them once on the held-out validation sets (voxconverse-test, msdwild); verify the sweep report gains validation columns for exactly those candidates and selection follows the documented rule (min held-out DER, no tuning component regressed >2pp)
- [x] 3.1a Extended grid (approved D1 amendment — grid-1 winner failed the held-out gate because ceiling 20 forces below-threshold merges): threshold {0.55,0.60} × ceiling {64,128} × gap-merge {0.0,0.3} over the tuning pools; verify the extended sweep report contains all 8 candidates with complete components
- [x] 3.2a Select the top-2 extended-grid candidates by mean tuning DER, evaluate each once on the held-out validation sets (voxconverse-test, msdwild); final defaults = best under the D5 rule across grid-1 + probe + extended evidence
- [x] 3.3 Set the winning values as the built-in Rust defaults (single constants/config), rebuild the release bin; verify default-asserting tests match the new values and `uv run --project eval subset` runs with them

## 4. Re-baseline and wrap-up

- [x] 4.1 Re-run full evaluation on all four datasets with tuned defaults and regenerate the comparison report; verify Conf dropped materially on real-data sets (voxconverse Conf < 25.3, msdwild Conf < 28.27, ru-youtube Conf < 36.67 — else stop and record the null result in the report before proceeding) and the report file shows the new numbers with Δ vs baselines
- [x] 4.2 Re-baseline the subset regression gate: record new expected metric ranges in `eval/README.md` and manifest comments; verify `subset` passes within the recorded ranges, and a deliberate bad override (e.g., threshold 0.9) makes the gate fail with a clear regressed-source message
- [x] 4.3 Run `openspec validate tune-diarization-clustering-params --strict` and document the three settings keys (names, defaults, effect) in the eval README and app settings docs; verify validation passes
