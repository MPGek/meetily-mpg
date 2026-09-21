# Proposal

## Why

After `05-unified-diarization-engine` the two diarization paths share one core, but the live path is still the weaker one and nobody can say by how much. In Fast mode the speaker labels a user sees are the polyvoice `StreamingPipeline`'s own incremental ids: `finalize` maps `channel.turns` straight into `SpeakerSegment`s (`frontend/src-tauri/src/audio/online_diarization.rs:1305-1330`) and never re-clusters, never runs segmentation-3.0, and never revisits an early decision made on two seconds of audio. The offline path, given the same recording, re-decodes it through ffmpeg and re-embeds every window even though the just-finished session already buffered per-chunk embeddings and handed them over (`audio/recording_commands.rs:1118-1141` moves `mic_raw`/`sys_raw` into `ONLINE_SESSION_DATA`). And the one number that describes the user's actual experience — how often the label shown live is not the label finally saved — is not measured anywhere.

## What Changes

- Add a **deferred re-clustering pass at recording stop** for Fast mode: cluster the session's buffered chunk embeddings with the shared `Clustering` implementation instead of trusting the streaming pipeline's incremental ids, then rebuild the channel timeline from the result.
- Define **label promotion**: labels shown during recording are provisional; at stop the refined timeline produces a final revision. By default the pass revises only blocks that were still provisional or unresolved and blocks whose cluster changed; a wholesale relabel is an opt-in setting.
- **Correction feedback**: binding a live cluster to a person forces a reconcile pass so already-displayed rows of that cluster re-emit a revision immediately, instead of only new rows carrying the corrected name.
- **Cache-reusing offline pass**: offline diarization of a meeting that was just recorded with online diarization reuses the session's buffered embeddings and turns and skips decode/segmentation when their coverage of the recording is adequate, falling back to the full pipeline otherwise.
- **`AudioSource` unification**: the streaming driver feeds the same core the batch driver feeds, so live and batch differ in their source and their clustering policy, not in their segmentation or embedding behavior.
- **Measurement**: add a live-versus-final label agreement metric to the evaluation tooling, computed from the streaming event sidecar and the finalized output produced by `add-online-diarization-eval`, and record acceptance gates for the online path.
- No new IPC command and no new event: the refined result is delivered through the existing revision mechanism of the live blocks stream and the existing stop-time assignment path.

## Capabilities

### New Capabilities
<!-- None: this change strengthens existing diarization and evaluation capabilities. -->

### Modified Capabilities
- `online-speaker-diarization`: adds an end-of-meeting refinement pass over the session's buffered embeddings, and defines live labels as provisional until that pass promotes them to final.
- `live-speaker-labels`: a user correction re-emits revisions for rows already displayed, not only for rows produced afterwards.
- `speaker-diarization`: offline diarization of a just-recorded meeting reuses the session's cached embeddings when their coverage is adequate, with an unconditional fallback to the full pipeline.
- `diarization-eval-scoring`: adds the live-versus-final label agreement metric and its recorded per-dataset bound to the regression gate.

## Impact

- Rust: `frontend/src-tauri/src/audio/diarization/` — `streaming/processor.rs` (stop-time refinement), `streaming/reconcile.rs` (revision re-emission on correction), `core/cluster.rs` (deferred policy behind the `Clustering` trait), `core/segment.rs` + a new `AudioSource` seam shared by `batch/` and `streaming/`, `persist/` (cache-reuse entry), `engine.rs` (facade surface unchanged in shape).
- Evaluation: `eval/src/diareval/scoring.py` and the metrics module added by `add-online-diarization-eval`, `eval/manifests/voxconverse-dev.yml` and `eval/manifests/ru-synthetic.yml` (gate bounds), `eval/README.md`, and the Rust online harness binary that change introduces.
- Prerequisites: `05-unified-diarization-engine` (the `Clustering` trait, the resolved parameter surface, and the `DiarizationEngine` facade) and the in-flight `add-online-diarization-eval` (the online harness mode, the streaming event sidecar, and the streaming metric set this change reuses rather than redefines).
- Out of scope: changing the offline defaults or the offline result for a recording that has no cached session, removing the Efficient engine (decided after the numbers exist), new datasets, new live events, and any schema change.
