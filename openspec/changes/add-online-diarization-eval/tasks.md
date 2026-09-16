## 1. Online capture seam

- [ ] 1.1 Add the optional all-emissions sink to `OnlineDiarizationProcessor` (carrying the stability flag next to each turn), passed as `None` by the production recording path; verify `cargo test online_diarization` passes and a new test asserts that when the sink is `None` the app-facing sender receives exactly the same turn sequence as before the change
- [ ] 1.2 Confirm the seam observes provisional emissions: with the sink attached, a chunk sequence that produces a provisional turn records it, while the app-facing sender still receives only final turns; verify with a unit test driving `process_chunk` over a fixture and asserting the two streams differ only by the provisional entries
- [ ] 1.3 Confirm headless model resolution and the single-process guard: constructing a processor with an explicit models directory succeeds, a missing enhanced model set fails with an error naming the files, and a second construction in the same process fails with the guard error; verify each case by test or CLI run

## 2. Online harness binary

- [ ] 2.1 Create the developer-only `online-eval` bin target (decode WAV, resolve models, feed the single canonical channel as `DeviceType::Microphone` with `has_system_device = false`, write the run header naming mode, chunking policy, and model family); verify it exits 0 on a bundled WAV and prints that header, and exits non-zero with a diagnostic on an undecodable input
- [ ] 2.2 Replay the production chunker: per-channel `ContinuousVadProcessor` over the decoded audio with the 200 ms dispatch window, the 500 ms gap and 25 s accumulation flush triggers, `merge_segments(.., 500.0, 25 * 16000)`, and the `VadConfig::live()` minimum-length filter, emitting `AudioChunk`s with absolute recording-relative timestamps; verify a unit test over a fixture with a known silence gap produces the expected merged chunk list and that no chunk timestamp exceeds the recording duration
- [ ] 2.3 Emit the streaming event sidecar: one record per emission in arrival order (absolute start/end, cluster label, stability flag, display provenance when present, emission index) plus a provenance header (mode, chunking policy, model family, sample rate, duration); verify the sidecar parses, every record carries all required fields, and its times are order-preserving
- [ ] 2.4 Emit the finalized RTTM for the same recording in the canonical offline shape; verify it loads under `pyannote.core.Annotation.load_rttm` and the existing scorer accepts the run directory without modification
- [ ] 2.5 Add the chunking-policy selector with the production-faithful policy as default and any alternative labelled as an ablation in the header and sidecar; verify a default run is marked production-faithful and a non-default run is marked an ablation
- [ ] 2.6 Prove determinism: two runs of the same recording with the same models and the same chunking policy produce byte-identical sidecar and RTTM; verify with a hash comparison over both artifacts

## 3. Parity with the app

- [ ] 3.1 Parity check (integration): run the same stored recording through the app's online flow and through `online-eval` on the same machine and models, then diff the emitted turns and the finalized assignments; verify the differences are within the streaming pipeline's ordering tolerance for deterministic configuration, and file a defect before continuing if they are not

## 4. Evaluation integration

- [ ] 4.1 Runner plumbing: select mode and chunking policy, resolve the matching harness binary, and scope run directories by mode so online and offline runs for the same dataset coexist; verify that running both modes on one dataset yields two independently addressable run directories and that re-running either without `--force` resumes instead of reprocessing
- [ ] 4.2 Scoring: score an online run with the identical no-collar, overlap-counted setup over the same UEM and attribute the result to the run's mode, refusing any score that would combine modes; verify on the existing synthetic fixtures that a hypothesis duplicating the reference scores DER 0 and a shuffled-speaker hypothesis scores Conf > 0 through the online path, and that a mixed-mode score attempt fails

## 5. Streaming metrics

- [ ] 5.1 Implement emission-lag computation from the sidecar against the dataset reference (earliest covering emission per reference turn, median and p90 in audio time, plus an uncovered count); verify with a synthetic sidecar and reference pair whose offsets are known exactly and compare the computed distribution against the expected values
- [ ] 5.2 Implement label flip rate, distinct runs per reference speaker, cluster-switch rate within a reference speaker's active speech, and live-versus-finalized fragmentation, aggregated per dataset by duration weight; verify against synthetic cases (one speaker emitted as alternating labels; a clean single-speaker stream) asserting the metric moves and stays put as expected
- [ ] 5.3 Capture real-time factor for the online path as its own recorded figure and keep it out of gate evaluation; verify a run records RTF alongside DER and lag, and that no gate consults it
- [ ] 5.4 Fail loudly on missing input: when an online run lacks the sidecar needed for a metric, the command exits non-zero naming the recording rather than reporting a zero or omitted metric; verify by deleting one sidecar from a completed run and re-running the metrics command

## 6. Reporting and gates

- [ ] 6.1 Report both modes per dataset: online DER alongside offline DER with the online-minus-offline delta, plus the streaming metric columns, and keep a single-mode dataset readable without implying a missing comparison; verify a two-mode report and a one-mode report each render correctly from scored runs
- [ ] 6.2 Extend the manifest schema's known-metric set with the online metrics and record online gate bounds per dataset; verify a manifest declaring an online bound loads and a manifest declaring an unknown metric is rejected with a clear message
- [ ] 6.3 Extend the subset command to evaluate the online bounds and fail with the dataset, metric, mode, and measured value; verify a deliberately tightened bound produces exactly that failure message and an unmodified bound passes

## 7. Measurement, docs, and wrap-up

- [ ] 7.1 Run the first online measurement over the existing regression subset and record the measured online baselines (DER delta, lag median/p90, flip rate, fragmentation) as gate bounds in the manifests with documented margin; verify the subset command passes against the recorded bounds on a re-run
- [ ] 7.2 Update `eval/README.md` and the affected manifest comments with the online commands, the chunking-policy rule (production-faithful only for parity claims), and the rule that real-time factor is reported but never gated; verify every documented online command reproduces from a fresh read of the README alone
- [ ] 7.3 Run `cargo test`, `cargo clippy`, and frontend lint/typecheck, plus `openspec validate add-online-diarization-eval --strict`; fix only failures introduced by this change
