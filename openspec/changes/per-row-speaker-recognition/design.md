# Design

## Context

See proposal.md — Why for the motivation and the measured numbers.

Current state that shapes the approach:

- `DiarizationEngine::persist_session` (`audio/diarization/engine.rs`) runs once per recording stop, after the frontend has created the meeting row. At that moment it still holds `OnlineSessionData`, whose `mic_embeddings`/`sys_embeddings` are the session's raw per-chunk embeddings as `(start_secs, end_secs, embedding)` — retained for exactly this kind of stop-time work.
- `persist/clusters.rs::persist_and_recognize_session` writes, per cluster, the centroid and a bounded exemplar cache (top 32 by duration) into `speaker_embeddings`, which already carries `audio_start_time`, `audio_end_time` and `channel` per row, then does one `best_match_with_threshold(&centroid, ..)` and one `set_auto_binding_if_unbound`.
- The displayed name comes from one shared SQL projection, `TRANSCRIPT_DISPLAY_SELECT` (`database/repositories/meeting.rs`): `COALESCE(so.name, s.name, t.speaker_label)` with a provenance `CASE` over `t.speaker_override_id` and `ms.matched_by`. Every transcript surface reads through it.
- On the measured meeting the persisted exemplar cache already covered 34 of 35 transcript rows with an overlapping embedding, so per-row evidence exists after the session too, not only at stop.

One pre-existing inconsistency is deliberately left alone: the `speaker-identity-registry` spec states a threshold of 0.7 while `TITANET_RECOGNITION_THRESHOLD` is 0.68. This change reuses whatever that constant is and does not touch either side.

## Goals / Non-Goals

**Goals**
- Decide a row's automatic name from evidence about that row, with no new tunable and no new matching algorithm.
- Keep one definition of the display precedence, in the shared projection, so every surface agrees.
- Leave a later re-match able to reproduce or retract the decision from persisted data alone.

**Non-Goals**
- Splitting the merged cluster (owned by `05b-live-diarization-accuracy`).
- Backfilling meetings recorded before this change.
- Any change to the live recording view, which already shows per-turn recognition.

## Decisions

### D1: Compute at stop from the session buffers; recompute later from the exemplar cache

Chosen: `persist_session` computes the row-level matches from `OnlineSessionData`'s raw per-chunk embeddings, which cover the whole session. `rematch_meeting_speakers` recomputes them from the persisted `speaker_embeddings` rows, whose `audio_start_time`/`audio_end_time` make "the embeddings overlapping this row" a plain query, and clears what the current candidate set no longer supports.

Alternatives considered: computing at stop from the exemplar cache only (rejected — the cache is bounded at 32 rows per cluster by duration, so a short row can be uncovered while its raw embedding was in hand); persisting every raw embedding so both paths share one source (rejected — that multiplies voiceprint storage for a benefit the coverage measurement does not show a need for; the cache already covered 34 of 35 rows).

Consequence: the two paths can disagree for a row whose raw embedding was not cached. That is acceptable and visible: re-match clears rather than invents, so the row falls back to its cluster.

### D2: Two nullable columns on `transcripts`, not a new table and not an existing column

Chosen: `speaker_auto_id TEXT NULL` (a `speakers.id`) and `speaker_auto_score REAL NULL`, added by one additive migration.

Alternatives considered: reusing `transcripts.speaker_label` (rejected — it is the manual label channel written by `update_speaker_label_command`, and it sits *below* the cluster binding in precedence, which is the wrong side); a side table keyed by transcript id (rejected — the relation is one-to-one with the row and the display projection already joins three tables; a fourth join buys nothing).

### D3: The precedence lives in the shared projection, and a user-bound cluster suppresses the row level

Chosen: extend `TRANSCRIPT_DISPLAY_SELECT` to `COALESCE(so.name, <row-auto name>, s.name, t.speaker_label)` where the row-auto name is suppressed when the cluster is user-bound, and extend the provenance `CASE` so a name resolved at the row level reports automatic provenance with `t.speaker_auto_score`.

Rationale: the requirement is about what a user sees, and every surface reads this projection, so encoding the rule anywhere else would let two surfaces disagree. Suppressing the row level under a user-bound cluster keeps the existing promise that an explicit user decision wins — including the "confirm this binding as correct" flow, which flips the cluster to `matched_by='user'` and therefore also settles every row of that cluster.

### D4: Reuse the existing matcher, threshold and channel preference verbatim

Chosen: `speaker_recognition::best_match_with_threshold` over the same candidate set cluster recognition uses (the meeting's expected speakers, or all speakers when the allowlist is empty), same `titanet_large` filter, same channel preference, same threshold constant.

Rationale: the point of this change is *which embeddings* are matched, not *how*. Reusing the matcher keeps the row level and the cluster level comparable, so a score means the same thing at both levels and the existing `(auto)` + percentage rendering stays truthful.

### D5: Both live modes, never the batch path

Chosen: the per-row pass runs wherever a live session buffered embeddings — Fast and Efficient alike — and never in offline diarization.

Rationale: the defect is specific to a live session's clustering being coarse; the offline path is the calibrated baseline whose numbers other work is gated against, and the specs state its behaviour is unchanged.

## Risks / Trade-offs

- [A row-level match is wrong where the cluster was right — a short or noisy row matching the wrong prototype] → Same threshold as cluster recognition, same-channel evidence only, and a user override or user-bound cluster always wins. The verification measures the real outcome on the measured meeting rather than assuming it: 10 of 18 wrong rows corrected, 9 rows agreeing with their cluster, 13 below threshold keeping today's behaviour.
- [Two levels of automatic naming make "why does it say that" harder to answer] → Provenance already travels with the name, and the score returned is the score of the level that won, so the surface can always explain itself.
- [A stale row-level name outranking a refreshed cluster binding after an allowlist edit] → Re-match refreshes and clears row-level matches in the same operation; the specs make that a requirement, not an implementation nicety.
- [Migration applied to a populated database] → Both columns are nullable with no backfill, so every existing meeting keeps resolving exactly as it does today.

## Migration Plan

One additive migration adds the two nullable columns; nothing is backfilled and no existing column changes meaning. Deploy is the normal app update: the columns stay NULL until a new recording stops, and old meetings are unaffected.

Rollback is reverting the code. The columns can stay behind harmlessly — with the old projection nothing reads them — so a rollback needs no down-migration.

## Open Questions

- Whether to offer a one-off backfill that computes row-level matches for meetings recorded before this change from their cached exemplars. Deferrable: it reuses the re-match path this change already builds, and answering it later changes neither the specs, the approach, nor the task breakdown.
