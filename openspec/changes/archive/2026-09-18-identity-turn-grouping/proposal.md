## Why

The `speaker-turn-display-grouping` merge keys on the raw diarization cluster label, but the same human is routinely split across several clusters that each auto-recognize to the same person name (visible in real recordings: same display name, different dot colors). Those clusters then render as separate un-merged rows, which is exactly the fragmentation the grouping change set out to fix. Once records are merged, the UI exposes only the turn's first member for relabeling; this change keeps that as an accepted limitation — an expand/collapse control was implemented for it and withdrawn on 2026-09-17 because its appearance and position churn were not wanted.

## What Changes

- The display turn-grouping key becomes the **resolved speaker identity** (registry person identified by a user binding/pin, or the person recognized via auto-match) instead of the raw cluster label; unlabeled clusters fall back to raw cluster label + `source_device` as before.
- Merged turns always render collapsed (concatenated member text in the source-side layout); no expand/collapse control is offered, so only the turn header (its first member) carries an editable speaker label.

## Capabilities

### New Capabilities
<!-- none -->

### Modified Capabilities
- `speaker-turn-grouping`: the merge identity key changes from the raw cluster label to the resolved speaker identity; a merge must not span rows whose resolved identities differ.

## Impact

- Frontend only: `frontend/src/lib/speaker-turn-grouping.ts` (identity resolver + grouping key change), `frontend/tests/lib/speaker-turn-grouping.test.ts`, and the merged-turn branch in `VirtualizedTranscriptView.tsx` (removal of the withdrawn expand/collapse control).
- The identity resolver consumes the same fields already present on transcripts (`speaker`, `speaker_label`, `speaker_matched_by`) — no backend, API, or persistence changes.
- Extends the in-flight (not yet archived) `speaker-turn-display-grouping` change; its tasks are verified behavior except where superseded by the identity key here.
