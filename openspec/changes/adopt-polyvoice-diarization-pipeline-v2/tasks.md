# Tasks — adopt polyvoice pipeline_v2 for offline diarization

## 1. Feasibility spike (resolves design D1 escape hatch + Open Questions)

- [x] 1.1 Enable the polyvoice `vbx` feature in `frontend/src-tauri/Cargo.toml` and verify `cargo check -p meetily` compiles with the pinned `ort` (2.0.0-rc.12)
- [x] 1.2 Spike: drive v2 components (binarized segmenter, dense `embed_window_secs` windows, resegmentation, `ClustererKind::Vbx`/`NmeSc`, Hungarian mapping) over the existing 5 s-overlap chunk loop on voxconverse-dev recordings; verify DER Conf improves vs tuned AHC baseline (18.53 on dev) and record wall-time factor vs the ≤3× budget — if posterior stitching fails, invoke the D1 escape hatch (clustering+overlap stages only) and update design.md/specs before continuing
- [x] 1.3 Spike: run the default kind on a single-speaker meeting and a <30 s clip; verify no degenerate speaker counts; record NmeSc quality on ru-youtube; capture the vendored default `BinarizationConfig` onset/offset values (0.5/0.5/0/0 — plain thresholding, so the spike must select hysteresis constants by probe) and the VBx/PLDA incompatibility (256-d-locked params, ~265 KB, CC-BY-4.0 pyannote-derived) into the spike notes
- [x] 1.4 Spike: measure peak working-set memory of the spike path on a >2 h recording and verify it stays within ~1.2× of the current chunked core's logged peak

## 2. VBx gating & model management (revised: no PLDA asset ships — D2 spike finding)

- [x] 2.1 Gate the `vbx` clusterer kind: the clusterer factory returns a clear actionable error naming the 256-d PLDA requirement for the enhanced 192-d family (no panic, no silent kind switch); verify no PLDA files are referenced by the build (`tauri.conf.json` resources, `build.rs`) and `cargo check` stays green
- [x] 2.2 Verify `check_diarization_models` and the engine agree on kind resolution: stored kind `vbx` → actionable error at diarization time; `nmesc`/`ahc` → success with the bundled enhanced set only
- [x] 2.3 Record the PLDA provenance/size/license finding (256-d-locked, ~265 KB, CC-BY-4.0) in the spike notes and design (done — no asset bundled, no `LICENSES.md` entry needed); verify settings-panel scenarios from the spec delta pass manually on a dev build

## 3. Core rework (`audio/diarization.rs`)

- [x] 3.1 Replace per-segment sparse embedding with dense `embed_window_secs` windows through the existing batched multi-core embed path; verify per-segment embeddings are the L2-normalized mean of that segment's windows (unit test: window count, ordering, normalization) and batch/fallback parity holds
- [x] 3.2 Switch segmentation to calibrated binarization (hysteresis + min-duration smoothing at the spike-selected constants) and accumulate frame posteriors across chunk overlaps; verify a low-confidence mid-speech dip no longer truncates a segment on a stored test clip
- [x] 3.3 Wire the clusterer factory (`vbx|nmesc|ahc` from `DiarizationConfig`) with the ceiling passed as max-speakers (clamped to 255 with a log); verify unit tests for kind resolution, clamp, and ahc-threshold-ignored-under-automatic-count
- [x] 3.4 Replace the app's post-clustering gap-merge pass with pipeline gap-fill (`max_gap_secs` from the existing `gap_merge_secs` setting, 0 disables); verify the two spec scenarios (same-speaker bridged, cross-speaker boundary preserved) in the existing diarization tests
- [x] 3.5 Implement overlap-aware output: two-speaker assignment via Hungarian local→global mapping, emitting temporally overlapping `DiarizationSegment`s; verify overlap and non-overlap scenarios from the speaker-diarization delta, and channel namespacing (`MIC_SPEAKER_NN`/`SPEAKER_NN`) unchanged
- [x] 3.6 Remove the singleton-cluster pruning pass; verify its tests are deleted and the ceiling still bounds label count on the fragmented-long-recording fixture
- [x] 3.7 Update transcript attribution for overlapping turns (token-level: larger covered duration wins; segment-level: max overlap unchanged); verify split/gap-fill scenarios in the "Speaker label assignment" spec still pass with overlapping input segments
- [x] 3.8 Keep `ClusteredEmbedding`/cache persistence intact (centroid = cluster mean, bounded exemplars from per-segment aggregates, enrolled prototypes untouched); verify the existing cluster-cache persistence and prototype-preservation tests pass, and stage-timing logs cover the new stages within the regression-warning knobs

## 4. Settings surface

- [x] 4.1 Add `diarizationClusterer` (default `ahc` per the 6.2 sweep) to the Rust `DiarizationConfig`, the startup mirror command, and `frontend/src/lib/diarization.ts` + `ConfigContext.tsx` localStorage persistence; verify app↔harness defaults match with no stored key and a stored kind survives restart
- [x] 4.2 Add a clusterer-kind selector to the diarization settings UI with an "(AHC only)" hint on the threshold control; verify manual check in the settings panel

## 5. Harness parity

- [x] 5.1 Add `--clusterer=<vbx|nmesc|ahc>` to `src/bin/diarize_eval.rs` and the sweep grid support (plus `--embed-window`/`--binarization` passthroughs from D4); verify sweep runs land per-candidate hypotheses as before
- [x] 5.2 Re-run the app-vs-harness parity check on a stored meeting with default flags; verify byte-identical RTTM output re-accepts the parity test on the new architecture

## 6. Eval re-baseline and gates

- [x] 6.1 Full-set runs (voxconverse, msdwild, ru-youtube, ru-synthetic) with the new default kind; verify the acceptance gate **msdwild Conf < 28.27** and no voxconverse/ru-youtube regression vs the 2026-09-05 report; record the measured wall-time factor vs the 3× budget
- [x] 6.2 If the default kind regresses the ru-* sets or misses the msdwild gate, sweep nmesc-vs-ahc (and gap/ceiling re-tune) on the tuning pools with held-out validation per the sweep protocol; verify the selected built-in default kind/values and document the decision in the report
- [x] 6.3 Re-baseline `subset_gate:` ranges in `eval/manifests/*.yml` and the README baseline/gate tables; verify `uv run --project eval subset` passes on the new defaults
- [x] 6.4 Update `eval/README.md` (sweep section for kind/window params, rollback note: `diarizationClusterer=ahc` + threshold 0.52 + gap 0.0 restores pre-adoption behavior) and `docs/CODEBASE_MAP_MODULE_AUDIO.md` for the v2 core; verify docs match shipped defaults

## 7. Integration verification

- [x] 7.1 Run `cargo test -p meetily` (or the crate's test invocation) and clippy; verify all diarization, cache-persistence, and attribution tests pass
- [ ] 7.2 End-to-end on a stored stereo meeting (auto + manual re-diarize, cancel mid-run, re-diarize after voiceprint enrollment): verify progress events, provenance/`matched_by='user'` guards, and that enrolled prototypes survive re-diarization on the new architecture
- [ ] 7.3 Run `graphify update .` and commit; verify the map reflects the reworked module boundaries
