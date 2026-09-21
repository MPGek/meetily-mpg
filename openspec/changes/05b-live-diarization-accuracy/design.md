# Design

## Context

See `proposal.md` for motivation. Verified current state (2026-09-18, branch `feat/diarization`), on top of what `05-unified-diarization-engine` establishes:

- **Fast mode never re-clusters.** `OnlineDiarizationProcessor::finalize` (`audio/online_diarization.rs:1267`) takes `channel.turns` — the polyvoice `StreamingPipeline`'s stable turns, pushed at `1170-1177` with `turn.speaker.0` as the speaker id — and maps them through `TimelineMapper::to_abs` into `SpeakerSegment`s (1305-1330). The buffered chunk embeddings (`mic_emb`/`sys_emb`, pushed at 1153) are used only for centroids and enrollment, never for clustering. Only the Efficient branch clusters (1291-1292).
- **Live labels have no promotion step.** The reconcile stage emits a revision per parent block when attribution changes (`audio/live_diarization_reconcile.rs:435-465`), driven by the watermark rule (`decidable`, 198) and the registry `Notify` (`changed`, 228). Nothing marks a block final, and nothing re-emits when the *identity* behind a cluster changes.
- **A correction does not touch what is already on screen.** `assign_live_speaker` (`audio/recording_commands.rs:2185`) calls `PrototypeStore::bind` (`online_diarization.rs:639`) for cluster scope, or pushes a `TurnOverride` for block scope. The display name is attached to a turn only when a *later* chunk produces a stable turn (`online_diarization.rs:1196-1214`), so already-emitted rows keep the old name until the frontend's own pinning rewrites them.
- **The session's embeddings are handed over and then dropped.** At stop, `mic_raw`/`sys_raw` move into `ONLINE_SESSION_DATA` (`recording_commands.rs:1118-1141`); `finalize_online_session` `take()`s that data (1812) and, after `persist_and_recognize_session` and enrollment, nothing keeps the per-chunk set. Only the centroid plus at most `MAX_CLUSTER_CACHE_EXEMPLARS = 32` exemplars per cluster reach the database (`diarization.rs:28`, `persist_channel_clusters` 2394).
- **Offline always decodes.** `start_diarization` (`diarization.rs:204`) goes through `spawn_ffmpeg_pcm` (2147) → `StreamWindows` (2221) → `V2Core` (1501) unconditionally, even for a meeting whose recording just finished.
- **The session already measures its own speech.** `OnlineDiarizationStats` tracks per channel the chunks received, embeddings ok/failed, and buffered milliseconds (`online_diarization.rs:193-335`), which is exactly the information a coverage test needs.
- **Evaluation.** `eval/src/diareval/scoring.py` scores a run directory of RTTMs against `data/<dataset>/rttm/ref.rttm` under `DiarizationErrorRate(collar=0.0, skip_overlap=False)` (line ~78) restricted by `data/<dataset>/uem/ref.uem`. Gates live in the manifests as `subset_gate: {metric: max}` and are enforced by `eval/src/diareval/subset.py:68-85`; `eval/src/diareval/manifests.py:117` currently accepts only `der|fa|miss|conf` as gate metrics. The regression subset is `voxconverse` (5 files) and `ru-synthetic` (5 files) — the two manifests carrying `subset: true`. `ru-synthetic`'s DER is inflated by a documented annotation-timeline artifact, so it is gated on `conf` only. The offline harness is `frontend/src-tauri/src/bin/diarize_eval.rs` (168 lines), which writes one RTTM line per `ChannelClusters::segments` entry; the in-flight `add-online-diarization-eval` adds the online counterpart and the streaming event sidecar.

## Goals / Non-Goals

**Goals:**

- Make the label a user ends up with better than the label the streaming pipeline produced on two seconds of audio, without changing what the live view looks like.
- Make the difference between the live label and the saved label a measured, gated number instead of an impression.
- Stop paying for a full decode + re-embed when the session that just ended already produced the embeddings.
- Keep one segmentation/embedding behavior across live and batch by feeding the same core from two sources.

**Non-Goals:**

- Changing offline results for a meeting with no cached session, or changing offline defaults.
- New live events, new IPC commands, or a schema change (the reusable cache is in-memory only).
- Redefining the streaming metric set — `add-online-diarization-eval` owns online DER, emission lag, streaming label flip rate, fragmentation, and RTF; this change adds exactly one metric on top.
- Deciding whether the Efficient engine stays (see assumptions).
- New datasets, and any gate derived from throughput.

## Decisions

### D1: Deferred re-clustering at stop, behind a setting, with an unconditional fallback

At stop, Fast mode clusters `mic_emb`/`sys_emb` through the `Clustering` factory from 05 (resolved kind, merge threshold, ceiling) and rebuilds the channel timeline by attributing each buffered chunk window to its refined cluster, then merging adjacent same-speaker windows with the resolved gap-merge window — the same `core/turns.rs` behavior the batch path uses. The streaming pipeline's own ids become a fallback, used when a channel has fewer than two buffered embeddings, when clustering errors, or when the setting is off.

- Why cluster the chunk embeddings rather than the pipeline's centroids: the chunk embeddings are the only per-channel set that spans the whole session and is produced by the same embedder the batch path uses (`online_diarization.rs:1112`), so the refined result is comparable with an offline run of the same audio.
- Alternative considered: run segmentation-3.0 over the recorded audio at stop. Rejected for this change — it is the full offline pass under another name, and D3 gets that benefit more cheaply when it is wanted.
- The setting exists so the pass can be turned off if the eval numbers say it is not worth its cost.

### D2: Promotion semantics — provisional by default, revise only what changed

Every live block carries a provisional state. At stop the refined timeline is re-attributed with the shared token/timeline attribution, and a final revision is emitted only for a block that (a) was never decided, or (b) whose refined cluster differs from the one it was displayed with. A `diarizationFinalRelabelAll` setting makes the pass re-emit every block. A block whose speaker the user set — cluster binding or per-turn override — is never revised by the pass.

- Why not relabel everything: the live view is a reading surface; a wholesale rewrite at stop looks like a bug to the user even when every label improved. Restricting the revision to changed blocks keeps the visible churn proportional to the actual correction.
- This is recorded as an assumption to confirm (A2).

### D3: Cache-reusing offline pass, in memory, with a coverage test

The engine keeps the finished session's per-channel embeddings and refined turns under the meeting id after `persist_session`, in a single-entry cache that is dropped when the next recording starts. Offline diarization of that meeting id reuses the cache when **all** of:

- the cached `model_tag` equals the currently resolved embedder's `model_tag`;
- the channel had zero embedding failures during the session (`OnlineDiarizationStats::embed_failed == 0`);
- `buffered_speech_secs / session_speech_secs ≥ 0.90` for that channel, both already tracked by the session telemetry.

Otherwise the full pipeline runs. The decision and its inputs are logged, so any result can be attributed. Reuse skips decode, segmentation, and embedding; it still runs clustering, turn assembly, transcript attribution, and the cluster-cache persistence, including the "replace only unassigned rows" rule.

- Why in memory and single-entry: persisting per-chunk embeddings is a storage design of its own (the database deliberately keeps only a centroid plus 32 exemplars per cluster). The dominant case — auto-diarization right after stop, and the user pressing "diarize" on the meeting they just recorded — is covered without one.
- Alternative considered: reuse the persisted exemplar cache. Rejected: 32 exemplars per cluster are a recognition cache, not a timeline; clustering from them cannot produce segment boundaries.

### D4: `AudioSource` unifies the two drivers

```rust
pub trait AudioSource {
    /// Next 16 kHz window with its absolute start in recording time.
    fn next_window(&mut self) -> Option<(f64, Cow<'_, [f32]>)>;
}
```

`batch::PcmWindows` wraps today's `StreamWindows` over the ffmpeg stream (`diarization.rs:2221-2283`); `streaming::VadChunks` wraps the VAD-filtered chunks the processor already receives. The core (`core/segment.rs`) consumes an `AudioSource` and emits embedding units; batch and live then differ in their source and their clustering policy only. The existing `parity_stream_vs_in_memory_core` test (`diarization.rs:3222`) is extended with the streaming source so the two drivers are asserted to agree.

- Risk acknowledged: the polyvoice `StreamingPipeline` and segmentation-3.0 use different boundary conventions, so the live path's *turn* boundaries can shift when this lands. That is why the eval gates exist and why D1 ships before D4.

### D5: Correction forces a reconcile pass

After `PrototypeStore::bind` succeeds, the engine notifies the live registry so `Reconciler::reconcile` re-evaluates held and already-emitted blocks of that cluster and re-emits a revision with the new display name and `matched_by = "user"`. No new event: it is the existing `live-transcript-blocks` revision mechanism.

- Why in the backend rather than the frontend's pinning: the frontend already pins a user label, but only for rows it can associate with the cluster; the backend knows the cluster of every emitted block and is the only place that can be exhaustive.

### D6: Metric definitions (the one metric this change adds)

Reused unchanged from `add-online-diarization-eval`: online DER, offline↔online DER delta, emission lag (median/p90 in audio time, plus an uncovered count), streaming label flip rate, distinct runs per reference speaker, live-versus-finalized fragmentation, and real-time factor.

**Added: live-versus-final label disagreement (`live_final_flip`).**

Inputs: the run's streaming event sidecar `S` (emissions with absolute start/end, cluster label, stability, emission index), the run's finalized hypothesis RTTM `F`, the dataset reference `R`, and the dataset UEM `U` — the same `R` and `U` the run's DER uses.

1. `L(t)` = the label of the emission in `S` with the highest emission index whose interval covers `t`; undefined where no emission covers `t`.
2. Restrict to `T = support(F) ∩ U` — finalized speech inside the annotated regions.
3. Map `L` and `F` into reference label space using each one's own optimal one-to-one mapping against `R` (`pyannote.metrics`' optimal mapping, the same one DER applies), so a pure cluster renaming is not a disagreement.
4. `live_final_flip = duration{ t ∈ T : L(t) defined ∧ map_L(L(t)) ≠ map_F(F(t)) } / duration{ t ∈ T : L(t) defined }`, as a percentage.
5. `live_uncovered = duration{ t ∈ T : L(t) undefined } / duration(T)`, reported alongside and never folded into (4).

Aggregation per dataset is duration-weighted over recordings. Implementation lands next to `score_dataset` in `eval/src/diareval/scoring.py` so it shares the reference/UEM loading and the mapping.

- Why duration-weighted rather than the per-word formulation ("fraction of finalized words whose last live label ≠ persisted label"): the harness has no ASR, so it has no words. Duration weighting over finalized speech measures the same thing the user experiences (how much of the meeting changed label) and is computable from the artifacts that exist. A per-word counterpart inside the app is not part of this change.
- Why the optimal mapping is applied first: without it, refinement that produces a better partition under different cluster numbers would score as a large disagreement, which is the opposite of what the metric is for.

### D7: Gates

Recorded in the manifests as `subset_gate` entries and enforced by `uv run --project eval subset`; `eval/src/diareval/manifests.py:117` gains the new metric names (`add-online-diarization-eval` task 6.2 opens that list; this change adds to it).

| Dataset (regression subset) | Gated metric | Bound |
| --- | --- | --- |
| `voxconverse` (5 files) | online DER − offline DER | ≤ +3.0 points absolute |
| `voxconverse` (5 files) | `live_final_flip` | ≤ 5.0 % |
| `ru-synthetic` (5 files) | online Conf − offline Conf | ≤ +3.0 points absolute |
| `ru-synthetic` (5 files) | `live_final_flip` | ≤ 5.0 % |

- `ru-synthetic` is gated on Conf, not DER: its manifest documents that the reference timeline tiles the file contiguously while the audio contains silence, inflating Miss by ~43 points, and its existing gate is already `conf: 16.0` for the same reason.
- `voxconverse-dev` is the tuning set (its manifest says never to select published results on it) and has no `subset: true`; it is used for the tuning and ablation runs in the tasks, not as a gate.
- Real-time factor is recorded in the report and never gated, as the streaming-metrics capability requires; the assignment's "RTF unchanged" is therefore an observation to record in task 6.3, not a bound.
- Bounds are first measured, then recorded with the documented margin, as the existing gate table in `eval/README.md:113-127` does.

## Risks / Trade-offs

- **Unifying boundary conventions can make live DER worse before it makes it better** (polyvoice streaming turns vs segmentation-3.0 hysteresis) → D4 ships after D1 and behind the same gates; if the gate fails on D4, the `AudioSource` change is reverted independently of the refinement pass.
- **The refinement pass costs time at stop**, exactly when the user is waiting for the meeting to save → Measured as its own figure in task 2.4 and bounded by the buffered-embedding count, not by recording length in audio samples; the fallback path is unconditional, so a slow or failing pass never blocks the stop.
- **A wholesale relabel at stop looks like a bug** → D2 makes the narrow revision the default and the wholesale one opt-in (assumption A2).
- **The in-memory reuse cache holds a session's embeddings longer than today** → Single entry, dropped when the next recording starts; the per-chunk set for a long meeting is the same allocation that already lived through the session, so peak memory is unchanged and only its lifetime grows.
- **The new metric depends on an artifact `add-online-diarization-eval` produces** → This change is explicitly ordered after it; the metric fails loudly on a missing sidecar rather than reporting zero (delta scenario), so a mis-ordered run cannot produce a falsely good number.
- **Gate bounds set on 5-file subsets are noisy** → The bounds are relative (online minus offline on the same files) rather than absolute, which cancels most dataset-specific noise, and the first measurement records the margin explicitly.

## Migration Plan

1. Deferred re-clustering behind a setting, default on, with the fallback path (D1). Measured before anything else changes.
2. Promotion semantics and the correction-driven reconcile pass (D2, D5) — display behavior only.
3. `AudioSource` unification (D4), with the parity test extended to the streaming source.
4. Cache-reusing offline pass (D3) and deletion of whatever the unification superseded.

Each step is gated by the eval run in its task group; a step that fails its gate is reverted on its own without unwinding the others.

## Open Questions

- Should the refinement pass also be available as an explicit "re-run speaker detection" action on a finished meeting, distinct from full offline diarization? Deferred: the cache-reusing offline pass already covers the same need through an existing button.
- Should the reuse cache survive an app restart (a small on-disk sidecar next to the recording)? Deferred until the in-memory version shows the reuse actually fires often enough to matter.

## Assumptions to confirm with the owner

- **A1 (assumed, confirm with owner): both Fast and Efficient engines stay for now.** Removing Efficient is considered only after this change's first measurement run, on the evidence of the online DER delta and the emission-lag figures for both.
- **A2 (assumed, confirm with owner): the end-of-meeting pass revises only provisional/unresolved blocks and blocks whose cluster changed**, with a wholesale relabel as an opt-in setting rather than the default.
- **A3 (assumed, confirm with owner): sequencing.** This change applies after `05-unified-diarization-engine` and after `add-online-diarization-eval`; the in-flight eval work is not rebased and its metric definitions are reused rather than restated.
- **A4 (assumed, confirm with owner): the gated datasets are the existing regression subset** (`voxconverse` + `ru-synthetic`), not `voxconverse-dev`, which is tuning-only data by its own manifest.
