## Why

Live speaker renames still revert a few seconds after being applied — including on old transcript blocks from minutes earlier, for both mic and system channels. The previous fix pinned labels but the guard is too narrow: a pinned block is only protected when the *incoming* turn belongs to its own cluster. Because the frontend re-matches every transcript against a cumulative turn list that still contains the stale pre-rename auto turn, any unrelated speaker turn re-triggers a re-match that resurrects the old `(auto) xx%` name. The underlying inconsistency — the live turn stream still advertising the old auto identity for a cluster the user already bound to a person — remains unfixed.

## What Changes

- **Unconditional pinning**: a transcript the user has assigned a speaker to is never re-matched by any future speaker-turn event (not just same-cluster turns). Only an explicit further user edit changes it.
- **Turn-stream rewrite on bind**: when a live rename is applied (single-block or cluster-wide), the in-memory live turn list is rewritten so every turn of the affected cluster carries the user's chosen name with `matched_by='user'` instead of the stale auto name. This makes live re-matching self-consistent and removes the stale-name resurrection for the whole cluster, not just the edited block.
- **Refactor live-label state**: extract the user-assigned cluster/name override into a small, explicit store (cluster → name) consulted during re-matching, replacing the ad-hoc `pinnedLabelsRef` mutation, so the re-match logic is correct and testable.
- **Improved fixer hygiene**: no behavior change to stop-time persistence (the `recording-stopped`/`speaker_assignments` path and per-block `speaker_override_id` remain authoritative in the DB).

## Capabilities

### New Capabilities

- *none*

### Modified Capabilities

- `live-speaker-labels`: The "User-assigned live labels are pinned" requirement changes from a same-cluster-only guard to an unconditional freeze on re-matching; "Live speaker rename takes effect immediately (Fast mode)" and "Per-turn speaker override during live labeling" gain the guarantee that past turns of the affected cluster also carry the user's name live (turn-stream rewrite).

## Impact

- **Frontend**: `contexts/TranscriptContext.tsx` (unconditional pin, turn-stream rewrite on bind, extracted live-override store), `components/VirtualizedTranscriptView.tsx` / `app/_components/TranscriptPanel.tsx` (unchanged wiring; possibly a small helper), and any test utilities for the live transcript state.
- **Backend**: no changes — the live stream rewrite is purely the frontend making its local state consistent with the backend bind it already performed (`assign_live_speaker` → `PrototypeStore::bind`).
- **Specs**: delta spec for `live-speaker-labels`.
- **No DB migration, no new dependencies.**
