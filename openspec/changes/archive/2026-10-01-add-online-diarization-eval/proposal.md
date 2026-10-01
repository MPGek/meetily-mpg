# Proposal: add-online-diarization-eval

## Why

The diarization evaluation harness measures only the offline batch path (`diarize_wav_samples`). The paths the app actually runs while recording — Fast-mode streaming diarization (`online_diarization.rs`) and the Efficient-mode stop-time clustering — have zero measured quality, so online regressions (label flicker, over-segmentation into many short speaker runs, split transcript blocks, emission delay) can only be judged by ear. The harness change deferred exactly this work: `add-diarization-eval-harness` lists "Evaluating the *online* streaming path (`online_diarization.rs`) — needs a chunked real-time harness" as a Non-Goal and as an Open Question gated on offline DER being trustworthy first. Offline DER is now trustworthy (subset gate re-baselined 2026-09-07, pipeline-v2 core + AHC defaults), so the deferred half is unblocked.

A second motivation is diagnostic. The visible symptom that prompted this — one person's utterance rendered as a stack of near-identical sub-rows in the live transcript — is a streaming-clustering artifact. Offline DER cannot see it: extra sub-runs of one speaker land partly in Confusion and are partly absorbed by the optimal label mapping, while the number of rows a user sees is not a DER component at all. Measuring the online path needs its own metric set, not just the same DER pointed at a different hypothesis.

## What Changes

- Add an **online mode** to the headless harness: decode a WAV, re-chunk it through the **production VAD/merge path** (the same silence-stripped speech chunks `AudioPipeline` feeds the online processor), and drive the production `OnlineDiarizationProcessor` per channel with no Tauri app, database, recording session, or voiceprint registry.
- Capture **two artifacts** per recording: the finalized RTTM in the existing canonical shape, and a **streaming event sidecar** recording every emitted live turn (audio-time start/end, cluster label, stability flag, revision, emission index).
- Add **streaming metrics** that offline DER cannot express: DER of the online output and the offline-vs-online DER delta, emission lag distribution measured in **audio time** (never wall-clock), label flip rate, fragmentation (distinct sub-runs per reference speaker), and real-time factor on a single documented measurement run.
- Extend scoring and the report with the **online columns** and a per-dataset **online gate** alongside the existing offline gate.
- Keep mode and chunking selectable from the harness command line so the fidelity seam is explicit and ablatable (production-replay chunking is the default; fixed-window chunking exists only as an ablation, never as the basis of a parity claim).
- Reuse the existing normalized datasets and the existing scorer unchanged for the finalized RTTM; this change adds no new datasets (a stereo microphone/system fixture for channel-isolation coverage is deliberately deferred).

## Capabilities

### New Capabilities

- `diarization-eval-streaming-metrics`: computes and reports streaming-only quality metrics from a captured online turn stream — emission lag, label flip rate, fragmentation, provisional/stable and revision counts, and real-time factor — and exposes them as gateable per-dataset metrics.

### Modified Capabilities

- `diarization-eval-runner`: the harness gains a selectable online mode that replays production VAD-merged chunking through the online diarization processor and writes a streaming event sidecar next to the RTTM; dataset orchestration can run either mode and reports which mode produced a run directory.
- `diarization-eval-scoring`: scoring and reporting gain online results — DER for the online run, the offline-vs-online delta per dataset, the streaming metric columns, and online gate keys evaluated by the regression subset command.

## Impact

- `frontend/src-tauri/src/bin/` — new online-mode entry point, either a second binary or a mode flag on the existing one; reuses the Tauri-free `OnlineDiarizationProcessor::new` constructor and the production VAD chunker.
- `frontend/src-tauri/src/audio/` — no behavior change expected; the chunker and the processor may need their entry points widened to be reachable from a headless binary.
- `frontend/src-tauri/Cargo.toml` — no new dependencies anticipated; streaming latency presets and `label_flip_rate` already ship in the pinned polyvoice version.
- `eval/src/diareval/` — runner mode plumbing and sidecar capture, a new streaming-metrics module, scoring/report columns, and additional manifest gate keys.
- `eval/README.md` and `eval/manifests/*.yml` — document the online commands and record the online gate baselines.
- Ordering constraint (satisfied): this change modifies capabilities introduced by `add-diarization-eval-harness`, which was archived on 2026-09-15; its main specs now exist, so these deltas apply normally.
- No shipping-app behavior change: the harness target stays developer-only and is never bundled.
