# Proposal

## Why

The offline speaker pass - the "Speakers" button, and the analysis that re-derives every automatic name after an Enhance - still names a whole cluster from one match of its centroid. On the user's meeting `Meeting 2026-09-30_18-06` that left a single name: live, every block had been named correctly from its own audio (that recording replays audio the registry had just been taught, which flatters the live result - see design D2); after Enhance and "Speakers", one merged cluster took "Vasiliy Kotov" for the block that is Alex Shingel as well as for Vasiliy's own, and the clusters of two other people scored 0.60 and 0.65 (their mean vector against a 0.68 threshold) and stayed anonymous. `per-row-speaker-recognition` fixed exactly this for a live session and deliberately left the offline path alone; the offline path is where every automatic name lands after an Enhance, so it is now the weaker of the two.

## What Changes

- After an offline diarization has persisted its clusters and cluster bindings, run the same row-level refresh the re-match operation already runs: each transcript row of the meeting is named from the meeting's persisted embeddings that overlap it (its own channel only, same candidate prototypes, same threshold, same-channel rule). A row with no usable embedding, or whose best candidate stays below the threshold, keeps resolving through its cluster. The offline pass and a later re-match therefore agree.
- Clear a meeting's earlier row-level matches before recomputing them, so a name recorded by a previous run (a live session, or an earlier offline pass) can never outrank the result of the run that just finished.
- A failure of the row-level step is logged and leaves the cluster bindings as the fallback; it never fails the diarization or changes its status.
- Update the test that recorded the previous decision ("the offline pass records no row-level match"): the cluster-persistence function still records none on its own, and the new step is what names rows.
- Not changed: clustering, turns, transcript speaker labels, cluster centroids and exemplar caches, DER, the cluster-level recognition, the user's per-block overrides and user-bound clusters, and the re-match operation. No schema change, no IPC command or event, no setting.

## Capabilities

### New Capabilities
<!-- None: this extends recognition that already exists. -->

### Modified Capabilities
- `speaker-identity-registry`: the per-row automatic recognition requirement stops being limited to a live session and now also covers an offline pass; a run replaces the meeting's earlier row-level matches.
- `speaker-diarization`: post-clustering recognition additionally names rows from their own embeddings, and its failure never fails the diarization.

## Impact

- Rust: `frontend/src-tauri/src/audio/diarization/persist/clusters.rs` (one small best-effort wrapper around the existing `refresh_transcript_row_matches`), `batch/orchestrator.rs` (one call after `persist_and_recognize_session`). No other production file.
- Data: writes only `transcripts.speaker_auto_id` and `speaker_auto_score`, columns that already exist.
- Evaluation: none. The eval harness (`diarize-eval`) never reaches persistence, so its output must stay byte-identical, which is checked on a sample against the recorded baseline.
- Out of scope: raising the exemplar cache cap (see design, open questions); changing the offline clustering that merged two people into one cluster; the live path; naming rows from the offline run's own in-memory embeddings, which was tried and dropped (see design D2).
