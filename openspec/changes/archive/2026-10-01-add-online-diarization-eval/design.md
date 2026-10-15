## Context

See proposal.md — Why. Current state that shapes the approach:

The offline harness exists (`frontend/src-tauri/src/bin/diarize_eval.rs`, decode → `diarize_wav_samples` → RTTM) and its Python driver locates `target/release/diarize-eval(.exe)`, spawns one process per WAV through a thread pool, and scores with `pyannote.metrics` at collar 0 / overlap counted (`eval/src/diareval/runner.py`, `eval/src/diareval/scoring.py`).

The online path is reachable headlessly. `OnlineDiarizationProcessor::new(mode, max_speakers, has_system_device, models_dir, turn_sender, prototype_store)` is Tauri-free (`new_with_app` is a separate wrapper), `process_chunk(AudioChunk)` is the only input, and `finalize(&[TranscriptSegment])` is the stop-time output. `AudioChunk` and `TranscriptSegment` carry no Tauri types. Two constraints come with it: a process-wide guard admits only one online processor per process, and the constructor refuses to build unless the *enhanced* models (segmentation-3.0 + TitaNet-Large, 192-d) are present.

The production chunking is a reusable recipe rather than something entangled with device capture: per channel `ContinuousVadProcessor::process_audio` yields `SpeechSegment { samples, start_timestamp_ms, end_timestamp_ms, .. }`; the pipeline accumulates them, flushes when a new segment is ≥500 ms beyond the pending tail or when accumulated speech would exceed 25 s, then `merge_segments(pending, 500.0, 25 * 16000)`, drops segments shorter than `VadConfig::live().min_segment_samples`, and builds `AudioChunk { sample_rate: 16000, timestamp: start_timestamp_ms / 1000.0, device_type, channels: 1 }` (`audio/vad.rs`, `audio/pipeline.rs`). VAD dispatch is windowed at 200 ms of input samples. The online processor re-expands compressed streaming time through its own `TimelineMapper`.

One discrepancy has to be settled before implementation. The streaming-metrics spec requires the event record to distinguish provisional from final emissions, but the processor forwards to `turn_sender` only inside `if turn.stable { .. }` — provisional turns never leave the processor today. The record cannot satisfy its contract by observing the existing sender.

## Goals / Non-Goals

**Goals:**
- Reproduce production online behaviour on a file, deterministically, with no Tauri/DB/session/registry.
- Capture the emitted turn stream faithfully enough that lag, flip, and fragmentation are computable without the app.
- Keep the calibrated offline path untouched and keep both modes independently runnable and scorable.

**Non-Goals:**
- Reproducing the live word-level split (sub-rows). That requires ASR tokens and the reconcile stage; the datasets ship no transcripts, and the split is display-only. Fragmentation is therefore measured on the emitted turn stream, not on rendered sub-rows.
- Efficient mode as a scored mode. It emits no turns; its stop-time output is keyed by transcript `sequence_id` and its per-chunk cluster spans are not part of its public surface. Measuring it would need either a transcript-free turn accessor or a paired ASR run.
- Stereo microphone/system fixtures: canonical datasets are mono, so channel isolation stays untested here.
- Wall-clock latency and any gate derived from machine speed.

## Decisions

### D1. A second binary rather than a mode flag on the existing one

Chosen: a sibling bin (working name `online-eval`) sharing only decode and model resolution with `diarize-eval`.

Alternatives considered: a `--mode` flag on `diarize-eval` (rejected — every existing flag there is an offline-pipeline knob: cluster threshold, clusterer kind, embed window, binarization, gap-merge; under `--mode=online` most become inapplicable and the argument matrix grows a second meaning); subcommands in one binary (rejected — cargo bins are cheap, and an enum dispatch layer buys nothing here).

Rationale: the two pipelines share no tunables, the offline numbers are the calibrated baseline that must not move, and separate binaries let the Python runner treat each mode as just another executable.

### D2. Reuse the production chunker verbatim; never reimplement it

Chosen: drive `ContinuousVadProcessor` + `merge_segments` + the min-length filter with the same constants (200 ms dispatch, 500 ms gap, 25 s cap, `VadConfig::live()`), and feed the resulting `AudioChunk`s to the processor.

Alternatives considered: reimplementing VAD/merge in the harness (rejected — guarantees divergence from the thing being measured); fixed-size windows (kept only as an explicitly labelled ablation, never the basis of a parity claim).

Fidelity nuance: production anchors VAD-relative times against a real capture clock because the device drops samples; a file replay has no drops, so VAD time maps one-to-one onto file offsets. The harness uses that no-drop case of the same mapping and verifies absolute times against the WAV duration.

### D3. Fast mode is the scored online mode

Chosen: online mode drives `DiarizationMode::Fast` and scores the streaming turns.

Rationale: Fast is the only mode whose output is a turn stream observable through the public surface (`turn_sender`). Efficient's turns exist internally as per-chunk cluster spans but never surface, and its public stop-time output is transcript-keyed. Scoring Efficient would require widening its public output for evaluation only — a change to the measured code, which is the one thing this harness must not do.

Consequence: `has_system_device = false` and the single canonical channel is fed as `DeviceType::Microphone`, producing `SPEAKER_NN` labels. This matches the mono datasets exactly.

### D4. Observe all emissions, including provisional ones — with a harness-only sink

Chosen: add an optional second sink to the online processor that receives every emitted turn together with its stability flag; production passes `None`, so the app's behaviour and payloads are unchanged.

Alternatives considered: recording only the stable stream and dropping provisional metrics (rejected — the spec requires provisional and final emissions to be distinguishable); changing the existing sender payload to carry stability (rejected — it would alter an app-facing event contract for evaluation's benefit).

Note that the guard/singleton and the registry publish inside `process_chunk` are process-global side effects. A harness process handles exactly one recording, so both are inert; the harness must not create two processors.

### D5. Capture in Rust, interpret in Python

Chosen: the binary emits the RTTM plus a per-recording event sidecar; every derived metric is computed in Python next to the existing scorer.

Rationale: the reference RTTM/UEM, DER setup, and dataset aggregation already live in Python; the spec requires the streaming metrics to be computed against the same reference and annotated regions as that DER, which is guaranteed by construction only if they are computed in the same place. Duplicating RTTM/UEM handling in Rust would create two definitions of "the reference".

Detail: polyvoice's `label_flip_rate` is frame-level over label arrays and does not fit our turn stream; the metric here is turn-level and computed in Python. The latency presets remain relevant only if preset selection is later exposed as an ablation knob.

### D6. Reference semantics: DER on the finalized set, lag on the emitted stream

Chosen: `der_online` is computed over the finalized turns (the last label per region — what a saved transcript would show); emission lag is computed over emissions as they arrived (earliest emission covering a reference turn). The two are reported side by side and never combined into one score, matching the "latency, RTF, DER as separate numbers" convention.

Alternative considered: a single tolerance-window score that folds lag into correctness (rejected — it hides which of the two moved, and makes the bound the subject of the measurement rather than the pipeline).

### D7. Mode-scoped run directories and manifest keys

Chosen: a run directory is identified by mode and run id, so online and offline hypotheses never collide, resume from each other, or mix in one score. Online metric bounds are declared per dataset with the metric name and mode, and the known-metric set in the manifest loader grows to accept them. Real-time factor is recorded but is never a gate.

### D8. Model resolution mirrors the offline harness

Chosen: same explicit `--models-dir` plus the search fallback, and the model family is recorded in the sidecar.

Rationale: online is 192-d enhanced-only while the offline harness can run either family, so a 192-vs-256 mismatch could silently make a cross-mode comparison meaningless. Recording the family makes that visible rather than assumed.

## Risks / Trade-offs

- [The chunker's flush cadence is part of the segmentation, not just the VAD] → Replicate the dispatch window and both flush triggers exactly; verify with a repeat-run determinism test and a chunk-list comparison against a logged production session.
- [Widening the processor with a second sink touches a measured code path] → The addition is inert in production (`None`); keep it a pure observation hook with no branching effect on emission, and cover it with a test that asserts the app-facing sender receives the same turns as before.
- [Streaming output depends on chunk boundaries, so an ablation chunking policy produces different numbers] → Ablation runs are labelled in the sidecar and are excluded from parity claims; only the production-faithful policy feeds a gate.
- [Timeline anchor drift in the mapper] → Feed strictly monotonic, non-overlapping chunk timestamps and assert final turn end ≤ recording duration.
- [Lag has no natural tolerance, so a "covered" turn may be covered badly] → Lag is defined as first covering emission and reported as a distribution (median/p90) with an uncovered count, never as a pass/fail scalar.
- [Online runtime is sequentially bound per file] → Keep file-level parallelism in the runner (one process per file, which the singleton guard already forces); do not attempt intra-file parallelism.
- [Fragmentation metrics invite tuning against the metric] → Report live-vs-finalized fragmentation separately so a fix that only hides the live symptom is visible as such.

## Migration Plan

Purely additive and rollbackable: a new developer-only binary, new `eval/` modules, and one inert optional hook in the online processor. No schema, database, or persistence change; no shipping-app behaviour change. Rollback is deleting the new binary target and the new `eval/` modules and reverting the hook. The offline harness, its baselines, and the existing subset gate are untouched; online bounds are recorded only after the first measurement run and are added, never substituted.

## Open Questions

- Whether to expose the polyvoice latency preset as an ablation knob alongside the chunking policy — deferred until at least one baseline exists, since it changes nothing in the specs or the task order.
- Whether Efficient mode ever becomes scorable, and if so whether via a turn accessor or via a paired ASR transcript — genuinely later work; it is a Non-Goal above.
- Which non-default chunking policy (fixed window vs raw VAD without merge) is the useful ablation — resolvable during implementation without changing the approach.
