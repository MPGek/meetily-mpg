## 1. Identity resolution in grouping helper

- [x] 1.1 Add `identityKey(row)` to `frontend/src/lib/speaker-turn-grouping.ts`: for resolved speakers the key is `<matched_by>:<speaker_label>`, falling back to `<speaker>:<source_device?>` when unresolved or unlabeled; switch `groupIntoTurns` and `withTurnGroups` merge comparisons to this key. Verify: the Node strip-types runner checks same-person/different-cluster merge, user-vs-auto same-name non-merge, and that unresolved fallback matches present behavior.
- [x] 1.2 Keep all existing break rules intact (listener-less purity, 60s gap, channel change, unresolved isolation) and re-run the existing `speaker-turn-grouping.test.ts` assertions converted to the identity key. Verify: updated unit tests pass; `npx tsc --noEmit` clean outside the known pre-existing `bun:test` errors.

## 2. Remove the expand/collapse control

The expand/collapse control was implemented (tasks 2.1-2.4, now withdrawn with the tasks) and is no longer wanted: its appearance and the control's position churn between the row's control cluster and a left-hand line were rejected on 2026-09-17. The requirement was removed from the spec delta and Decisions 3/4 were dropped from design.md, so merged turns always render collapsed.

- [x] 2.1 Remove the control from the merged-turn branch of `VirtualizedTranscriptView`: delete the toggle, its per-row `expanded` state, the `expandedMembers` body, and the `ChevronDown`/`ChevronUp` imports if they become unused, so a merged turn always renders its concatenated member text in the source-side layout; keep the turn's play/timestamp cluster and the word-level sub-row rendering unchanged. Verify: no merged turn in either view offers a toggle, no toggle-related code remains by inspection, typecheck and the Node run of `speaker-turn-grouping.test.ts` stay green, and `transcripts.json` is untouched. *(Toggle, `expanded` state, `expandedMembers` and the `ChevronDown`/`ChevronUp` imports removed; the branch now returns the collapsed rendering only, so the System cluster is back to `play, timestamp` as in standard rows. Grep for `expanded|expandToggle|Chevron` is clean; typecheck and lint show only pre-existing findings; 25/25 Node assertions pass. The on-screen check is part of 3.2.)*

## 3. End-to-end validation

- [x] 3.1 Extend `frontend/tests/lib/speaker-turn-grouping.test.ts` with the identity-key scenarios (same person across different clusters merges; user vs auto same name does not cross-merge) and update the older same-raw-key expectations. Verify: tests pass via Node runners (bun unavailable) and tsc is clean.
- [ ] 3.2 Manual check in both views: a recording where one person appears as several auto-matched clusters renders as their merged turns, a merged turn offers no expand/collapse control, and nothing about `transcripts.json` changes. Verify by observing the running app.
