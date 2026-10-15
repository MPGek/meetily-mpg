# Design

## Context

See proposal.md - Why. Current mechanics, as read from the code:

- A live split block renders its sub-rows from the backend `live-transcript-blocks` payload. All sub-rows share the parent transcript id and differ only by `[start, end]`. Each sub-row's `matched_by` comes from the live turn that best overlaps it (`reconcile.rs::provenance`), so sub-rows of one cluster can start out `auto`, `user` or `None`. The confirm checkmark renders only for `auto`.
- A sub-row edit or confirm calls `applyLiveSpeakerLabel(cluster, name, transcriptId, start, end)` (`TranscriptContext.tsx`). It makes three writes: a window override (`windowOverridesRef`), a pin keyed by transcript id (`userAssignmentsRef`, holding one `{cluster, name}` per transcript), and a turn rewrite inside the window.
- `resolveLiveBlocks` (`live-speaker-labels.ts`) resolves each sub-row in this order: a window override of the same cluster, then the pin if `pin.cluster === sub-row cluster`, then a cluster binding, then the backend value. The pin therefore covers every same-cluster sibling, and each edit on another cluster overwrites it, which reverts the siblings and brings their checkmarks back.
- `userAssignmentsRef` also serves as the freeze set that `rematchTranscripts` skips.
- The backend receives only the window override. `assign_live_speaker` with `scope="block"` pushes a `TurnOverride` without deduplication. At stop, `persist_session` (`engine.rs`) calls `enroll_embeddings_from_buffer` once per override. That call inserts the top-K chunk embeddings overlapping the window, each with a fresh UUID. Chunk embeddings span about 20 s while sub-rows span 1-3 words, so every sub-row click in a block re-enrolls the same chunks.
- Every path that assigns a prototype to a person goes through `SpeakerRepository::enforce_prototype_cap` inside its transaction: cluster-cache enrollment, block enrollment, buffer enrollment and reconfirm. The cap prunes the shortest `duration_secs` first.
- Measured on a copy of the user's DB: Vasil Boika has 64 rows and 15 distinct voiceprints, all from one meeting and created within 25 s at stop. The duplicates are byte-identical in embedding with the same window, and their `cluster_label` values differ.

## Goals / Non-Goals

**Goals:**
- The resolution order makes a sub-row edit sub-row scoped (spec: live-speaker-labels).
- No enrollment path can create a duplicate voiceprint, and existing duplicates are removed once (spec: speaker-identity-registry).

**Non-Goals:**
- Changing the offline (saved meeting) editor. Split sub-rows exist only in the live view.
- The mismatch between a 1-word sub-row and a ~20 s enrolled chunk that may contain another voice. This is a real quality risk, recorded under Risks and left to a separate change.
- Restoring distinct prototypes the cap already evicted because of duplicates.
- The `(auto) 90%` label seen without a checkmark on one row of the user's screenshot. Both come from the same `matchedBy === 'auto'` condition. It is tracked as a reproduction task, not a design point.

## Decisions

### D1. Split pinning from freezing
`userAssignmentsRef` currently means both "frozen from re-match" and "pinned name for this cluster". Introduce a separate frozen-id set for re-match. A sub-row edit adds the parent id to the frozen set and records a window override, and never writes the pin. An edit on an unsplit block keeps the pin and the freeze and no longer records a window override (see D2), so the "Split does not revert a pinned label" scenario still holds.

Whether an edit targets a sub-row is decided by the caller. The combobox already passes the sub-row window, but an unsplit block passes its own window too, so the window alone cannot tell them apart. `applyLiveSpeakerLabel` gets an explicit `subRow` flag from the sub-row render path in `VirtualizedTranscriptView`.

*Alternative:* key the pin per `(transcriptId, cluster)`. That keeps the sibling spillover the user rejected (option B), so it was not chosen.

### D2. Resolve overrides by window and channel, latest first
`resolveLiveBlocks` matches an override to a sub-row by time overlap on the same channel. The cluster test is dropped, so a re-attribution to another cluster label keeps the user's name. The override list is searched newest-first, so a later edit of the same sub-row wins without the exact-window dedupe the current filter relies on. Overrides gain `sourceDevice`, taken from the parent transcript.

Because the cluster test used to stop an override from reaching a neighbouring sub-row after a boundary shift, each override now lands on the single sub-row it overlaps most (found during implementation). For the same reason, an unsplit-block edit records only the pin, not a window override (see D1): a whole-block window would otherwise land on the largest sub-row after a split, whatever its cluster.

*Alternative:* keep the cluster test and re-key overrides when a revision changes clusters. That needs a mapping the frontend does not have.

### D3. Deduplicate inside `enforce_prototype_cap`
Before counting, delete a speaker's rows that duplicate an earlier row of the same speaker on `(meeting_id, channel, audio_start_time, audio_end_time, embedding)`, comparing NULLs with `IS`. The row kept is the one ranked first by `is_verified DESC, audio_blob IS NOT NULL DESC, created_at ASC`. If the kept row has no clip and a deleted copy does, the clip columns are copied over first. Because every enrollment path already calls this helper inside its transaction, one change covers all of them, and duplicates are gone before the cap decides what to prune.

In addition, `enroll_embeddings_from_buffer` filters out candidates that already exist for the speaker before cutting clips. This saves ffmpeg work and keeps its returned count honest.

*Alternative:* a UNIQUE index on the tuple. A blob-inclusive unique index is heavy, it would turn duplicate inserts into errors on paths that update `speaker_id` on cache rows, and it cannot be created until existing data is clean. The helper achieves the same guarantee without those costs.

### D4. Deduplicate overrides before the stop-time loop
In `persist_session`, collapse `turn_overrides` for enrollment per `(speaker_id, channel)`: gather the candidate chunks overlapping any of that person's windows, deduplicate them by chunk window, and enroll the union once, still capped at best-K per window. Applying overrides to transcripts (`apply_turn_overrides`) keeps using the full ordered list, so the latest override still wins there.

D3 alone would already prevent duplicate rows. D4 keeps the stop path from cutting the same clip N times and makes the log counts meaningful.

*As implemented:* the union turned out unnecessary. The D3 pre-insert check already runs before clip cutting and each override's enrollment commits before the next, so a chunk shared by several overrides is cut and stored once. The stop loop was extracted into `enroll_override_ground_truth` and skips only exact repeats of a `(speaker, channel, window)`. Per-window best-K selection is unchanged.

### D5. One-time migration for existing duplicates
Add a SQL migration that applies the D3 rule to every speaker. It collapses duplicates with the same keep-ranking and clip carry-over, and it touches only rows with `speaker_id IS NOT NULL`. Unassigned cache rows are left alone, since duplicate cache rows are not a person's voiceprints and are pruned by existing purge tools.

## Risks / Trade-offs

- [A sub-row edit no longer renames same-cluster siblings, which some users may have liked] → This is the chosen behaviour (option A). Apply-to-all remains the way to rename the whole cluster.
- [Migration deletes rows irreversibly] → It deletes only byte-identical copies of a row that is kept, and the kept row inherits the verified flag and clip. The task list verifies it on a DB copy first.
- [Blob comparison cost in the dedupe query] → It is scoped to one speaker, with at most about 64 + K rows, and pre-filtered by meeting, channel and window before comparing blobs.
- [Enrolled ~20 s chunk may contain another speaker's voice] → Out of scope (Non-Goals). Worth a follow-up change that enrolls only chunks mostly covered by the corrected window.
- [Parent transcript frozen by a sub-row edit shows a stale parent label if the block later collapses to one run] → Low impact. The parent label is hidden while the block is split. Noted, and not handled.

## Migration Plan

1. Ship the D5 migration together with D3, so the app never runs with duplicates and without the guard.
2. Before merging, run the migration on a copy of the user's DB and compare per-speaker counts against the measured baseline (Vasil Boika 64 → 15).
3. Rollback: the code changes are revertible. Deleted duplicate rows are not restored, and nothing is lost by that, because each one was a copy of a row that was kept.
