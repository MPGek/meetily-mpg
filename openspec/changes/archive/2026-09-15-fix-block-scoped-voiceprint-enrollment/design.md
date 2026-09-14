## Context

Offline speaker correction currently routes every scope through one enrollment primitive. `assign_block_speaker` (`frontend/src-tauri/src/database/speaker_commands.rs`) sets a per-transcript override and then calls `SpeakerRepository::enroll_cluster` (`database/repositories/speaker.rs`), which reparents the cluster's top-K=8 cache rows by `duration_secs`. It ignores which block the user edited. `assign_speaker` and `apply_block_speaker_to_cluster` use the same primitive, where whole-cluster enrollment is the intended behavior.

A window-scoped primitive already exists: `enroll_embeddings_from_buffer` filters candidate embeddings by time-window overlap and is used only by the online finalize path (`audio/recording_commands.rs`) for single-turn overrides. Offline has no in-memory session buffer, but it does have the persisted cluster cache in `speaker_embeddings` with `audio_start_time`/`audio_end_time` provenance, plus the corrected block's `source_device` on the transcript row (already used to pick a channel in `assign_block_speaker`'s NULL-cluster branch).

See `proposal.md` for motivation and the delta specs for the behavioral contract.

## Goals / Non-Goals

**Goals:**
- Single-block corrections enroll and clean up only the audio overlapping the corrected block, on the block's channel.
- Repeated re-corrections of the same block converge: the newest speaker owns the block's audio, and no other speaker keeps it.
- Cluster-wide bindings ("apply to all") remain whole-cluster but stop leaving stale prototypes from the same cluster with the previous speaker.
- No schema change and no audio re-processing.

**Non-Goals:**
- Retroactive cleanup of voiceprints already contaminated by earlier releases.
- Changing how offline/online diarization produces clusters or how auto-recognition scores them.
- Reworking the online single-turn enrollment path, which is already window-scoped.

## Decisions

### D1: Add a DB-backed, window- and channel-scoped enrollment primitive
Add a repository method (e.g. `enroll_block_window`) that, for a `(meeting_id, cluster_label, channel)` and a block window, selects up to K=8 unassigned cache rows whose `audio_start_time < block_end` and `audio_end_time > block_start`, ordered by `duration_secs` descending, and reparents them to the speaker.

- Rationale: mirrors the overlap predicate already used by `enroll_embeddings_from_buffer`, but operates on the persisted cache instead of a session buffer, so offline corrections need no audio re-processing.
- Alternative considered: call `enroll_embeddings_from_buffer` directly — rejected because offline has no session buffer in memory and that method inserts new rows, whereas enrollment is reparenting of cache rows (design D2 of the registry).
- Alternative considered: re-run diarization for the block — rejected: expensive and unnecessary, the cache already holds per-segment embeddings and timecodes.

### D2: Order cleanup before enrollment, so re-corrections converge
For a single-block correction on cluster C / channel X / window W and speaker P:
1. Demote to unassigned cache any prototype row from `(M, C, X)` that overlaps W and belongs to a speaker Q ≠ P, unless a transcript block overridden to Q also covers that row.
2. Enroll the cache rows overlapping W to P (D1), capped at K.

- Rationale: because enrollment only claims unassigned rows, a second correction of the same block would otherwise find nothing to enroll (the first correction already reparented those rows). Demoting first makes the newest correction win.
- The "pinned by a per-block override" exception protects a legitimately corrected sibling block when exemplar windows and transcript windows overlap at their edges.
- Demote-to-cache (not delete) reuses the voiceprint-rejection semantics: the row stays reviewable and re-enrollable.
- Alternative considered: reparent Q's overlapping rows straight to P — rejected because a single 17s exemplar can partially overlap W; leaving it unassigned lets the K-cap and later corrections decide, rather than forcing a wrong full-segment move.

### D3: Cluster-wide bindings demote stale same-cluster prototypes
When a cluster is bound cluster-wide to speaker S (`assign_speaker`, `apply_block_speaker_to_cluster`, and live cluster renames at finalize), demote any prototype row from `(M, C, X)` that belongs to a different speaker and is not pinned by a covering per-block override, *before* running the best-K cluster enrollment.

- Rationale: the reported data shows a speaker accumulating prototypes from a cluster that was later re-bound elsewhere (and `meeting_speakers` diverging from the prototype set). Cluster-wide binding is a whole-cluster statement, so rows explicitly corrected to a block still win, but unattributed leftovers must not stay with the old owner.
- Demote-before-enroll also lets the re-bound speaker receive the cluster's full best-K seed set instead of only the rows the previous owner had not claimed.
- Reuse the same cleanup helper as D2 to avoid two implementations.

### D4: Channel is derived from the corrected block's source device
Enrollment and cleanup are always scoped to one channel. The channel comes from the transcript's `source_device` ("System" → `system`, otherwise `mic`), consistent with the existing NULL-cluster branch in `assign_block_speaker`. Cluster labels are channel-namespaced (`MIC_SPEAKER_*` / `SPEAKER_*`) but the query filter is still explicit so a numeric index collision cannot leak across channels.

### D5: Keep `enroll_cluster` for cluster-wide scope
Cluster-wide paths keep `enroll_cluster` (best-K by duration). Only single-block scope moves to D1, and both scopes gain the D2/D3 cleanup. This keeps the change surgical and preserves existing cluster-wide behavior.

### D6: No-cache and legacy meetings
All new operations are no-ops when no cache rows match, and must not error; the label mapping is applied regardless, per the existing "correction with no audio still labels" behavior.

## Risks / Trade-offs

- [Removing whole-cluster enrollment from single-block corrections reduces how many prototypes a speaker gains per correction] → Correctness over volume: contamination is worse than a smaller seed set; the 64-prototype cap and repeated corrections accumulate good rows over time.
- [Overlap-based demotion can touch a legitimate row at a block boundary] → Demote to cache instead of deleting, and never demote a row that a current per-block override covers; the row stays reviewable in the Voiceprint Browser.
- ["Apply to all" on a mixed cluster still contaminates, because that path enrolls the whole cluster] → Intentional: cluster-wide is an explicit whole-cluster statement; the default stays single-block, and per-block corrections are the accurate path. Surfacing a mixed-cluster warning is out of scope.
- [Existing contaminated voiceprints are not repaired] → Non-goal; the user can reject/reassign in the Voiceprint Browser or re-correct the blocks, which now demotes-and-re-enrolls correctly.
- [Two write passes (demote then enroll) are not atomic across steps] → Run both inside a single transaction so a failure cannot leave rows demoted without enrollment.

## Migration Plan

- No schema or data migration. Deploy the code change; existing rows keep their provenance.
- Rollback: revert the code. Rows written by the new logic (fewer, correctly scoped prototypes) remain valid under the old code, so rollback needs no data fixup.
- Verification on the affected recording (`Meeting 2026-09-14_15-01`): re-correct the `SPEAKER_02` blocks and confirm each speaker's prototype set only contains rows whose timecodes fall in their own blocks, and that `SPEAKER_02` no longer contributes seven foreign exemplars to Alex Shingel.
