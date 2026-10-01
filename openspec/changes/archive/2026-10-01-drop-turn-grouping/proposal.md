## Why

The display-level merging of consecutive same-speaker transcript records ("turn grouping") plus its expand/collapse control were built as uncommitted work in this worktree and are not wanted: readers should see every transcript record as its own row, as before the feature. The grouping capability never reached the main specs (both contributing changes are unarchived drafts), so dropping the feature changes no durable requirement — it restores the per-record rendering that `split-transcript-ui` already specifies.

## What Changes

- Stop merging consecutive same-speaker records in both transcript views: remove the display grouping so every transcript record renders as its own row again.
- Remove the merged-turn branch and its (already withdrawn) expand/collapse control rendering from `VirtualizedTranscriptView`.
- Delete the grouping helper `frontend/src/lib/speaker-turn-grouping.ts` and its tests `frontend/tests/lib/speaker-turn-grouping.test.ts` (grouping, identity key, and the row side rule all live there).
- Drop the `turnMembers` field from `TranscriptSegmentData` and the `withTurnGroups` calls from both transcript panels.
- Keep the live word-level diarization sub-row rendering as is; the channel-side rule for a split record's sub-rows (previously only applied inside merged turns) moves to the `live-word-level-diarization` change as its own requirement and task.
- Retire the two superseded changes (`speaker-turn-display-grouping`, `identity-turn-grouping`) via `openspec archive --skip-specs`, so their deltas never reach the main specs.

## Capabilities

### New Capabilities
<!-- none: no new capability. -->

### Modified Capabilities
<!-- none: the grouping capability never reached the main specs; removing the feature changes no existing requirement. -->

This change opts out of specs with `skip_specs: true` in `.openspec.yaml`: it reverts uncommitted, never-specced work, and `split-transcript-ui` already describes per-record rendering.

## Impact

- Frontend only: `frontend/src/lib/speaker-turn-grouping.ts` (deleted), `frontend/tests/lib/speaker-turn-grouping.test.ts` (deleted), `frontend/src/app/_components/TranscriptPanel.tsx`, `frontend/src/components/MeetingDetails/TranscriptPanel.tsx`, `frontend/src/components/VirtualizedTranscriptView.tsx`, `frontend/src/types/index.ts`.
- No backend, API, or persistence changes; stored records and `transcripts.json` are unaffected.
- Superseded changes end up in `openspec/changes/archive/` without contributing spec requirements.
