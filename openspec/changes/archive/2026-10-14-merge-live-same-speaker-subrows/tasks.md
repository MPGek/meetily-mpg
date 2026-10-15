# Tasks

## 1. Merge logic (`src/lib/live-speaker-labels.ts`, `src/types/index.ts`)

- [x] 1.1 Add optional `parts?: { speaker: string; start: number; end: number }[]` to `LiveTranscriptBlock` and verify `bunx tsc --noEmit` passes in `frontend/`
- [x] 1.2 Add `mergeLiveBlocks(blocks)`.
  - Rules (design D2): merge adjacent blocks with equal `display_name ?? speaker` and equal user ownership. The merged block takes start of first / end of last, joined text, the longest part's `speaker`, the lowest `match_score`, and `parts`. Parts of already-merged inputs are flattened.
  - Verify with new tests in `tests/lib/live-speaker-labels.test.ts`:
    - three clusters with one name merge into one;
    - "Mikhail/Marina/Mikhail" stay three;
    - unnamed different clusters stay separate;
    - user vs auto of one name stay separate;
    - the longest part's cluster and the lowest score are kept;
    - an unmerged block is returned unchanged (same object).
- [x] 1.3 Split `resolveLiveBlocks` into exported `applyClusterAssignments` (pins + bindings) and `applyWindowOverrides` (`resolveLiveBlocks` = both, no merging, same behaviour), and add `resolveMergedLiveBlocks` = cluster assignments → merge → window overrides → merge (design D3).
  - Verify all existing `resolveLiveBlocks` tests still pass unchanged.
  - Add `resolveMergedLiveBlocks` tests:
    - an override made with a merged window renames the whole merged row on the next revision;
    - a rename that equalises neighbours merges them;
    - a pin on cluster A does not rename a neighbouring cluster-B part that had the same auto name;
    - the spec scenario A, B, A ("Alice (auto)", "Carol (auto)", "Alice (auto)"), where a first-row edit leaves the third alone.
- [x] 1.4 Add a pure `liveAssignmentCalls(parts, cluster, scopeAll, start, end)`. It returns the backend calls for an edit (design D5):
  - one apply-to-all call per distinct cluster;
  - one block-scoped call per part, with the part's window;
  - one call for a row without parts.
  - Verify with tests for a 2-part merged row (scoped and apply-to-all) and a plain row.

## 2. Wiring (context, panel, label editor)

- [x] 2.1 In `TranscriptContext.getLiveBlocksFor`, return the merged result whenever the raw payload has ≥ 2 blocks, including a single merged run; keep `undefined` for a raw single block. Verify with `bun test tests/` and `bunx tsc --noEmit`.
- [x] 2.2 In `TranscriptPanel`, when the result has one block, set the segment's label fields from that block, pass its `parts`, and leave `blocks` undefined (design D4).
  - Verify by type check.
  - Verify by reading the rendered row in the live view during task 3.1.
- [x] 2.3 In `VirtualizedTranscriptView`, pass `parts` to `SpeakerLabel` for sub-rows and collapsed rows. In the live branch of `handleAssign` and `handleConfirm`:
  - issue the calls from `liveAssignmentCalls`;
  - then apply locally: one window override over the merged window for a single-row edit (a collapsed row goes through the sub-row path), or `onUpdate` per distinct cluster for apply-to-all.
  - Leave the saved-meeting (`meetingId`) branch unchanged.
  - Verify by type check, `bun test tests/` and `bun run lint`.

## 3. Verification in the app

- [ ] 3.1 Record a Fast-mode session with diarization on (one speaker talking long enough to be split into several clusters, as in the screenshot). Check:
  - a block whose runs show one name renders as one row or one sub-row;
  - different speakers still render separate sub-rows.
- [ ] 3.2 In the same session:
  - rename a merged sub-row for that row only, confirm another, and rename one with apply-to-all;
  - each change shows on the whole row immediately;
  - after stop, the saved meeting shows the chosen names on the stored rows those sub-rows covered.

## Workflow follow-up

- Run `graphify update .` after the code changes.
- Archive the change after review.
