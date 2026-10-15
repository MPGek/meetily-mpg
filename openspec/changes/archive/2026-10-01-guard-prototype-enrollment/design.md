# Design

## Context

Enrollment promotes unassigned cache rows (`speaker_id IS NULL`) to prototypes by setting `speaker_id`, in four places in `database/repositories/speaker.rs`: `enroll_cluster` (cluster-wide, best-K by duration), `enroll_block_window` (rows overlapping a corrected block), `enroll_embeddings_from_buffer` (live chunk buffer, direct insert), and the reconfirm command (single row chosen by the user). Recognition reads only `speaker_id IS NOT NULL` rows, so caches of other meetings never influence a new diarization; prototypes always do. See proposal.md - Why for the measurements.

Measured on a copy of the user's database (2026-10-01):
- cache exemplar vs its cluster centroid: median 0.85, p10 0.62, 4% below 0.5; the longest 20% are the most coherent (median 0.91 vs 0.83), so ranking by duration is not the problem;
- prototype vs the mean of the same person's other prototypes: median 0.85, p10 0.60, 7 of 151 below 0.5, 25 of 151 nearer another person's mean.

## Goals / Non-Goals

**Goals:**
- Keep clearly incoherent rows from becoming prototypes through automatic selection.
- Make existing doubtful prototypes visible so the user can verify or reject them.

**Non-Goals:**
- Auto-deleting or auto-demoting existing prototypes.
- Fixing a cluster that is wrong as a whole (the guard compares candidates with each other, so a cluster that is mostly another voice passes).
- Changing the recognition threshold, the cache writer, or stale/orphan cache cleanup.

## Decisions

**D1. The guard compares candidates with each other, not with the target person.** The user chooses the person; a guard against the person's existing prototypes would block legitimate corrections of a wrongly named earlier meeting. The leave-one-out mean of the other selected candidates is the reference. *Alternatives:* cluster centroid from `meeting_speakers` (missing for orphan caches and for the buffer path, so it cannot be the single rule); similarity to the person's prototypes (blocks corrections, rejected above).

**D2. One shared pure function.** `filter_coherent(embeddings, threshold)` in a small module next to `speaker.rs` (cosine, mean and L2 normalize already exist in `identity::matching`). Each automatic path selects a pool larger than K (best-K by duration plus a few spares), runs the function, and takes the first K that pass. Fewer than three candidates are not judged (a mean of one vector says nothing).

**D3. Threshold 0.5, one named constant for both the guard and the flag.** It cuts the bottom ~4% of cache exemplars and the 7 clearly wrong prototypes while leaving the p10 (0.60-0.62) alone. A looser value would drop normal intra-speaker variation (laughter, distance from the microphone); a tighter one would reject legitimate samples. Revisit against the same measurement after use.

**D4. Reconfirm is explicit and unguarded.** A user who picks a row and a person has decided; the guard applies only to automatic best-K selection. The result still reports the row's similarity so the UI can warn (non-blocking).

**D5. The suspect flag is computed on read, not stored.** `list_voiceprints` already loads prototypes per speaker; embeddings are 192 floats and there are 151 rows today, so the computation costs milliseconds and needs no migration or invalidation when prototypes change. Rule per prototype of a person with at least three: suspect if its cosine to the mean of the person's other prototypes is below 0.5, or if its cosine to another person's full mean exceeds its cosine to its own leave-one-out mean. `VoiceprintRow` gains `suspect: bool` and `own_similarity: Option<f32>` (false/None for caches).

**D6. Flag, never remove.** The two rules disagree with the truth in both directions (two people with similar voices, a user-verified correct sample). Removal stays a user action through the existing Reject; Verify marks the row acknowledged.

## Risks / Trade-offs

- [Two genuinely similar voices flag each other] -> flag only; verified rows show as acknowledged; no automatic action.
- [A mostly-wrong cluster passes the guard] -> out of scope; the nearer-other-person rule catches it later as a flag.
- [Guard drops a rare legitimate sample (cough, far field)] -> it stays in the cache and can be reconfirmed by hand (D4).
- [Threshold tuned on one user's data] -> a constant, and the measurement is rerun in task 4.1 so it can be checked on other databases.
- [Per-open cost grows with the corpus] -> O(prototypes x people); with 64 prototypes per person and a few dozen people this is far below a frame; compute each person's mean once per call.

## Open Questions

- Whether the buffer path (`enroll_embeddings_from_buffer`) should report dropped candidates through the live UI or only the log.
