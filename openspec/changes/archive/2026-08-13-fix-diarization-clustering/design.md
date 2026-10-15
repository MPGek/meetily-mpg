## Context

Meetily's diarization runs polyvoice's powerset segmentation + ResNet34 embedding + AHC clustering. Both the offline engine (`audio/diarization.rs`) and online Efficient mode (`audio/online_diarization.rs`) construct the clusterer with `AhcClusterer::default()` or `AhcClusterer::new(max)`, which select a merge threshold **automatically** via polyvoice's "largest-gap" heuristic (`estimate_threshold_from_matrix`, clamped to 0.2–0.7).

That heuristic is fragile for real meetings. On a 7-minute, multi-person stereo recording it produced:

- system channel: 97 segments → 5 "unique speakers" (one dominant cluster + 4 noise fragments)
- every transcript matched to the dominant cluster, which AHC canonicalizes as label `0`

Because AHC relabels clusters by descending size, the dominant cluster always becomes `0` → `SPEAKER_00` → "Speaker 1" in the UI. The `"N unique speakers"` log line is misleading: it counts noise fragments, not distinct voices.

The polyvoice library itself never uses auto-threshold in its shipped pipelines. `ClusterConfig::default()` and `pipeline_v2` both use `AhcClusterer::with_threshold(max_clusters, threshold)` with `Profile::Balanced.default_threshold()` = `DEFAULT_AHC_THRESHOLD` = **0.45** — a value calibrated to the same ResNet34 embedding space this app uses.

## Goals / Non-Goals

**Goals:**
- Make offline and online Efficient-mode clustering use the calibrated fixed threshold so distinct speakers get distinct labels.
- Apply the user's `maxSpeakers` setting as a hard ceiling in both paths.
- Keep the `SPEAKER_NN` / `MIC_SPEAKER_NN` label scheme and matching logic unchanged.

**Non-Goals:**
- Changing the segmentation model, embedder, or channel-separation scheme.
- Tuning threshold per-recording or exposing a threshold knob in settings.
- Reworking Fast mode (`StreamingPipeline`), which does not use `AhcClusterer::default()`.
- Improving the `"N unique speakers"` log message (tracked separately if desired).

## Decisions

### Decision 1: Fixed threshold 0.45 instead of auto-threshold

Replace `AhcClusterer::default()` / `AhcClusterer::new(max)` with `AhcClusterer::with_threshold(max, DEFAULT_AHC_THRESHOLD)`.

Rationale: this is polyvoice's own shipped default for the Balanced profile (`types::config::DEFAULT_AHC_THRESHOLD = 0.45`, asserted in its tests and used by `ClusterConfig::default()`, `PipelineConfig::default()`, and the CLI clusterer factory). The ResNet34 embeddings are calibrated to merge clusters at cosine similarity ≥ 0.45, and the auto-threshold path exists only as a legacy/alternative, not the recommended production path.

Alternatives considered:
- **Tune the auto-threshold heuristic** — rejected: no stable knob, and it is exactly the failure path.
- **Switch clusterers (KMeans/NmeSc)** — rejected: larger behavior change, and AHC with the fixed threshold is the validated default.
- **Hand-roll a threshold** — rejected: reuse the library constant instead of duplicating a magic number.

### Decision 2: Wire `maxSpeakers` as the cluster ceiling

`AhcClusterer::with_threshold(max_clusters, threshold)` treats `max_clusters == 0` as "no ceiling" and otherwise hard-caps the cluster count.

- Offline: derive `max_clusters` from the existing `max_speakers: Option<i32>` parameter (`None`/`<= 0` → 0 = no ceiling).
- Online Efficient: `EmbeddingBuffer::cluster` already receives `max_speakers: usize`; keep passing it through, using 0 as "no ceiling".

This removes the current dead `maxSpeakers` setting (offline passed `None` when 0; online hardcoded `0` in `recording_commands.rs`).

### Decision 3: Min-cluster pruning enabled (min_size = 2)

Wrap the fixed-threshold `AhcClusterer` in polyvoice's `MinClusterSizeClusterer` with `min_size = 2` in both offline and Efficient-mode paths. Real-meeting evidence shows the fixed threshold fragments the system channel into singleton noise clusters (`SPEAKER_12`, `SPEAKER_19`), which this dissolves by reassigning each singleton to the nearest larger cluster — matching `ClusterConfig::default()` (`min_cluster_size: 2`).

### Decision 4: Gap-fill fallback for short unmatched utterances

The powerset segmenter (10s window / 2s hop) misses very short isolated speech bursts (1–2.5s), leaving those transcripts with no overlapping turn and thus no speaker. Add a nearest-neighbor fallback to `find_best_speaker` (offline and online):

- **Single-speaker channel**: assign that channel's single speaker unconditionally (correct for the local mic user's brief responses even across long silences).
- **Multi-speaker channel**: assign the temporally nearest turn's speaker only when within 30 seconds, otherwise leave NULL (avoid crossing speaker boundaries).

## Risks / Trade-offs

- **Fixed threshold may over- or under-merge on unusual audio** → The 0.45 value is the model vendor's calibrated default for this exact embedding space; worst case is the status quo, and it can be tuned later without a schema change.
- **Pruning may merge a genuine short minority speaker** → `min_size = 2` only dissolves single-segment clusters; any real speaker with two or more segments survives.
- **Gap-fill could mislabel across a long silence on a multi-speaker channel** → Bounded to 30s; single-speaker channels (the common mic case) are safe because there is only one possible label.
- **Regression on single-speaker meetings** → The `Single-speaker recording stays single-labeled` scenario guards this; a fixed threshold does not split a single homogeneous voice.
- **maxSpeakers behavior change** → Users who set a value will now actually see it applied; users who left it at 0 (default) are unaffected (no ceiling).

## Migration Plan

No data migration. Existing meetings already diarized with the degenerate labels can be corrected by re-running "Re-analyze Speakers" (manual offline diarization), which will now produce distinct labels. Rollback is a revert of the two clusterer-construction edits.

## Open Questions

- Whether to also surface min-cluster-size as a settings knob in a follow-up (deferred).
- Whether to rename the misleading `"Diarization found N segments with M unique speakers"` log line to distinguish "cluster count" from "estimated speaker count" (deferred).
