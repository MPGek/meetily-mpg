# Proposal

## Why

In the live recording view, editing or confirming one sub-row of a split transcript block renames other sub-rows of the same cluster, a later edit on another sub-row reverts them to `(auto)`, and confirm checkmarks appear and disappear on rows the user never touched. Each of those extra clicks is also recorded as a per-turn override, and at stop every override enrolls the chunk embeddings overlapping its window without checking for an existing copy: on a copy of the user's database, "Vasil Boika" holds 64 voiceprints (the per-person cap), only 15 of them distinct, all from one meeting, and six other people carry 2-7 duplicates each. Because the cap evicts the shortest prototypes first, the duplicated ~20 s chunks crowd out distinct voiceprints.

## What Changes

- A sub-row edit or confirm in a live-split block applies to that sub-row only. It records a window-scoped override and no longer creates or replaces the block-level pin, so it cannot rename, un-pin or re-check sibling sub-rows. A block-level pin made while the block was unsplit keeps covering every sub-row of its cluster after a later split (unchanged).
- A window-scoped override is resolved onto the sub-row by time window and channel, not by cluster, so a re-attribution that moves the span to another cluster label does not revert the user's choice. When several overrides cover a sub-row, the most recent one wins.
- A sub-row edit still freezes its parent transcript against live re-matching, without pinning a name onto the parent's cluster.
- Stop-time ground-truth enrollment from per-turn overrides enrolls each chunk embedding at most once per person, however many overrides overlap it.
- Enrollment never inserts a voiceprint for a person that is identical (same meeting, channel, audio window and embedding) to one that person already holds.
- A one-time migration removes existing exact duplicate voiceprints, keeping one row per (person, meeting, channel, window, embedding). Assumption to confirm at review: the user did not choose between this and manual cleanup; the migration was the recommended option.

## Capabilities

### New Capabilities

_None._

### Modified Capabilities

- `live-speaker-labels`: "Per-turn overrides and pinned labels survive live splits" — sub-row edits are sub-row scoped, overrides resolve by window rather than cluster, latest override wins; "Per-turn speaker override during live labeling" — override enrollment is deduplicated per chunk.
- `speaker-identity-registry`: new requirement that a person never holds two identical voiceprints, covering enrollment and the cleanup of existing duplicates.

## Impact

- Frontend: `src/contexts/TranscriptContext.tsx` (`applyLiveSpeakerLabel`, pin/freeze bookkeeping), `src/lib/live-speaker-labels.ts` (`resolveLiveBlocks`), `tests/lib/live-speaker-labels.test.ts`.
- Backend: `src-tauri/src/audio/diarization/engine.rs` (stop-time override enrollment loop), `src-tauri/src/database/repositories/speaker.rs` (`enroll_embeddings_from_buffer`), a new migration under `src-tauri/migrations/`.
- Data: existing duplicate rows in `speaker_embeddings` are deleted. Prototypes already evicted by the cap because of duplicates cannot be restored.
- No IPC signature changes.
