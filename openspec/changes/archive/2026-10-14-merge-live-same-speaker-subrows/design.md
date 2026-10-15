# Design

## Context

See proposal.md (Why). Current live sub-row path:

```
backend reconcile.rs: tokens -> group_into_blocks (by i32 cluster) -> LiveBlock{speaker, display_name, matched_by, match_score}
   -> apply_bindings (backend cluster bindings) -> emit `live-transcript-blocks` {parent_sequence_id, revision, blocks}
frontend TranscriptContext.getLiveBlocksFor(transcript):
   payload.blocks.length < 2 -> undefined (row renders with its segment-level label)
   else resolveLiveBlocks(blocks, {clusterBindings, pinned, windowOverrides, sourceDevice})
TranscriptPanel -> VirtualizedTranscriptView: blocks.length > 1 -> one surface, one labelled sub-row per block
```

`resolveLiveBlocks` (`src/lib/live-speaker-labels.ts`) gives each block, in precedence order, a window override (landed on the block with the largest overlap), else the block pin when its cluster matches, else the cluster binding.

The label editor (`SpeakerLabel` in `VirtualizedTranscriptView.tsx`) sends one backend call per edit for the row's `speaker` (cluster):
- `assignLiveSpeaker(cluster, ...)` for apply-to-all;
- `assignLiveSpeakerBlock(cluster, start, ..., end)` for one row.

It then calls `applyLiveSpeakerLabel` for the local state. At stop, `apply_turn_overrides` sets `speaker_override_id` on stored rows `WHERE speaker = cluster AND` the row overlaps the window. A per-row override therefore only reaches stored rows of the cluster it was sent for.

## Goals / Non-Goals

**Goals:**
- One sub-row per run of the same visible speaker, computed where the visible name is decided.
- Every existing assignment kind keeps working on merged rows, including persistence at stop.

**Non-Goals:**
- Any backend, IPC payload or revision change.
- Merging across parent transcript rows.

## Decisions

### D1. Merge in the frontend, next to name resolution

The visible name of a sub-row is final only after `resolveLiveBlocks` applies the session's window overrides, pins and cluster bindings. All three live in frontend refs, and window overrides exist only there. A backend merge would see backend bindings only: rows the user renamed to the same name would not merge, and a backend merge would also change the revision payload compared in `reconcile`.

Alternative rejected: merge in `reconcile.rs` before emitting.

### D2. Merge key: visible name plus ownership

Two adjacent blocks merge when `display_name ?? speaker` is equal and `matched_by === 'user'` is equal. The match score is not part of the key: the screenshot rows show 90 %, 92 % and 70 % for one name.

Alternatives rejected:
- Name only. It would hide a user confirmation behind an `(auto)` neighbour, or the reverse.
- Name plus score. Nothing would ever merge.

A merged block gets:
- `start` of its first part and `end` of its last;
- `text` of the parts joined with one space;
- `speaker` of the longest part by duration, which keeps the dot colour stable;
- the shared `display_name` and `matched_by`;
- `match_score` = the lowest part score, so the label never overstates confidence;
- a new optional field `parts: {speaker, start, end}[]`.

A block that merged nothing has no `parts`, or one part, and renders exactly as before.

### D3. Order: cluster assignments, merge, window overrides, merge

```
raw blocks
  -> apply pins + cluster bindings (per cluster: exact on raw blocks)
  -> merge (D2)
  -> apply window overrides (each lands on the merged block it overlaps most)
  -> merge (D2)                      (neighbours a rename made equal)
```

Why not resolve then merge:
- An override made on a merged row has the merged window.
- On the next revision it would land on only one raw part. That part would get the new name, its neighbour would keep the old one, and the row would split again.

Why pins and bindings must come before the first merge:
- They are per cluster.
- Applied after the merge, a pin on cluster A would rename a merged row that also contains cluster B, which the pin never covered.

`resolveLiveBlocks` keeps its signature, precedence and its no-merge behaviour; it becomes `applyWindowOverrides(applyClusterAssignments(blocks))`, with both halves exported. A new `resolveMergedLiveBlocks` runs the order above, and the context uses it. Its existing tests stay valid as tests of name resolution. A window override still beats a pin or binding, because it is applied last.

### D4. A single merged run is returned and rendered as the row's label

`getLiveBlocksFor` returns the merged list whenever the raw payload had two or more blocks, including a result of length 1. A raw payload of one block still returns `undefined`.

`TranscriptPanel` treats a one-element result as a label source:
- the segment gets that block's `speaker`, `display_name`, `matched_by` and `match_score` as its label fields, plus its `parts` for editing;
- `blocks` stays `undefined`, so the row renders as an ordinary row.

### D5. Edits fan out per part

`SpeakerLabel` gets an optional `parts` prop. In the live branch:
- Apply-to-all calls `assignLiveSpeaker(cluster, …)` once per distinct part cluster, then `onUpdate(cluster, name)` once per distinct cluster.
- Single-row edit or confirm calls `assignLiveSpeakerBlock(part.speaker, part.start, …, part.end)` once per part. Locally it records one window override with the merged window, through the existing sub-row path (`subRow = true`).

A collapsed ordinary row (D4) uses the same window-override path, not the block pin:
- A pin names one cluster.
- On the next revision it would rename only that cluster's part and split the row again.

The list of backend calls is computed by a pure helper in `live-speaker-labels.ts`, so it is unit-tested with the rest of the merge logic. The component tests here do not cover `SpeakerLabel`. Rows without `parts` behave exactly as today. The saved-meeting branch (`meetingId` set) is untouched, because live blocks exist only while recording.

## Risks / Trade-offs

- [Rows merge and split as names change] A re-attribution or a rename can turn three sub-rows into one or the reverse. → This is the same revision-driven re-render the split already has; no extra mitigation.
- [An earlier single-part override now covers a merged row] An override made on one part before the parts merged lands on the merged unit and renames all of it. This needs a part that was shown alone and then merged, i.e. a rename of a neighbour to the same name. → Acceptable: the user's latest visible intent is one row of that name.
- [Several backend calls per edit] A merged row of n parts sends n IPC calls. → n is small (a block is ≤ 25 s). Each call is the existing idempotent assignment; a failure shows the existing error toast, and the local label is not applied.
- [Lowest score shown] A merged row can show 70 % when most of its text scored 92 %. → Conservative by choice; the score is a hint, not a gate.

## Migration Plan

No data, settings or payload migration. Rollback is reverting the change.
