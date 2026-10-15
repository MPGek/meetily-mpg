## 1. Refactor live-override state

- [x] 1.1 In `contexts/TranscriptContext.tsx`, introduce a `userAssignmentsRef: Map<string, { cluster: string; name: string }>` keyed by transcript id, plus a `clusterBindingsRef: Map<string, string>` (cluster label в†’ name) for apply-to-all binds; keep both as refs and add small helpers `pinTranscript(id, cluster, name)` / `bindCluster(cluster, name)` that centralize mutation
- [x] 1.2 Move all mutation of the assignment map out of the `setTranscripts` updater into the helpers, so the ref is mutated synchronously before any state update (removes the fragile mutating-ref-inside-updater pattern)
- [x] 1.3 Rename/replace the old `pinnedLabelsRef` usages with the new helpers; update `clearTranscripts` and the `recording-started` reset to clear both maps

## 2. Freeze pinned transcripts in re-match

- [x] 2.1 In the `onSpeakerTurn` handler (currently `contexts/TranscriptContext.tsx` ~lines 138-160), replace the `if (pin && pin.cluster === turn.speaker) return t;` guard with an unconditional freeze: `if (userAssignmentsRef.current.has(t.id)) return t;`
- [x] 2.2 Confirm unpinned transcripts still re-match via `matchSpeakerToTranscript` (unchanged path), so the retroactive label fill-in for non-user-assigned blocks is preserved

## 3. Rewrite the turn stream on bind

- [x] 3.1 Add a helper in `TranscriptContext.tsx` that rewrites `turnsRef.current` in place: for a given cluster label and name, map every turn with `turn.speaker === clusterLabel` to `{...turn, display_name: name, matched_by: 'user', match_score: undefined}`; return the new array
- [x] 3.2 In `applyLiveSpeakerLabel` (line ~649), when called with no `transcriptId` (apply-to-all), call the rewrite helper for the whole cluster and record `clusterBindingsRef`; when called with a `transcriptId` (single-block), rewrite only turns overlapping that transcript's `[audio_start_time, audio_end_time]` window and pin just that transcript
- [x] 3.3 Ensure the rewrite runs after the backend `assign_live_speaker` bind succeeds and before the derived `setTranscripts` update, so the order is: backend bind в†’ turn-stream rewrite в†’ frontend state update

## 4. Unit tests for the live label logic

- [x] 4.1 Extract the re-match + rewrite logic into pure, testable helpers (if not already) and add a test that a transcript pinned to `SPEAKER_01`/"Alice" keeps its label when an unrelated `SPEAKER_02` turn arrives and `turnsRef` still contains a stale `(SPEAKER_01, "Bob", auto)` turn
- [x] 4.2 Add a test that after a cluster bind `SPEAKER_01 в†’ "Alice"`, `matchSpeakerToTranscript` against the rewritten `turnsRef` yields display name "Alice" and `matched_by:'user'` for turns of that cluster (idempotence: running it twice gives the same result)
- [x] 4.3 Add a test that a single-block edit rewrites only the overlapping turn(s) and leaves other turns of the cluster unchanged

## 5. Verification

- [x] 5.1 `tsc --noEmit` in `frontend/` passes for the changed files (only the pre-existing `bun:test` type error may remain)
- [x] 5.2 `npm run lint` (or the project's lint command) passes for the changed files
- [x] 5.3 Manual recording (Fast mode): rename a live speaker on an old block, wait >10s with other participants talking, and confirm the block keeps the user's name and never shows `(auto)` again; repeat for a cluster-wide rename and a single-block rename, on both mic and system channels
