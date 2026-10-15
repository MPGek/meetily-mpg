## Why

When offline diarization runs after online enrollment (the normal flow after recording stops), `write_cluster_cache` deletes ALL `speaker_embeddings` rows for each cluster — including enrolled prototypes (rows with `speaker_id IS NOT NULL`). This silently wipes out voiceprints that were just enrolled from user-assigned blocks during the recording session.

**Observed behavior**: User marks 5 speakers during a live meeting. Logs show 16 embeddings enrolled across 5 speakers. After offline diarization completes (~39 seconds later), only 1 speaker retains prototypes. The other 4 speakers' voiceprints are gone.

**Root cause**: The DELETE query in `write_cluster_cache` (`speaker.rs:284`) filters only by `meeting_id` and `cluster_label`, without checking `speaker_id IS NULL`. When offline diarization re-persists cluster caches, it deletes everything including prototypes.

## What Changes

- Fix `write_cluster_cache` to only delete cache rows (`speaker_id IS NULL`) when refreshing a cluster's exemplar set, preserving enrolled prototypes
- Add a regression test that verifies prototypes survive a subsequent `write_cluster_cache` call on the same cluster

## Capabilities

### New Capabilities

None — this is a bug fix to existing behavior.

### Modified Capabilities

- `speaker-diarization`: The "Cluster embedding cache persistence" requirement implicitly assumes cache writes do not destroy enrolled prototypes. The fix makes this explicit: `write_cluster_cache` SHALL only replace unassigned cache rows, never enrolled prototypes.

## Impact

**Affected code**:
- `frontend/src-tauri/src/database/repositories/speaker.rs` — `write_cluster_cache` DELETE query
- `frontend/src-tauri/tests/` — new regression test

**Affected behavior**:
- Offline diarization re-runs will no longer wipe out voiceprints enrolled during the recording session
- Manual "Re-analyze Speakers" on a meeting will preserve existing prototypes while refreshing cluster caches
- No API changes, no database schema changes

**Risk**: Low — the fix narrows a DELETE query to its intended scope (cache rows only). Enrolled prototypes were never meant to be deleted by cache refreshes.
