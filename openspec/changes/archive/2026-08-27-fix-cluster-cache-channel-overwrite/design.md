## Context

The `write_cluster_cache` function in `speaker.rs` persists cluster exemplar embeddings to the `speaker_embeddings` table. It currently deletes all rows matching `(meeting_id, cluster_label)` before inserting new ones, regardless of the `channel` parameter. This violates the channel-scoped cache independence requirement.

The bug manifests when:
1. Online diarization runs with `saw_system_audio=true`, creating both mic and system channel caches
2. The function is called again (e.g., due to a race condition or re-processing) with `saw_system_audio=false`
3. The system channel caches get overwritten with `channel='mic'` values

## Goals / Non-Goals

**Goals:**
- Fix the DELETE statement to include a `channel` filter, ensuring channel-scoped cache replacement
- Preserve existing behavior for all other aspects of cluster cache persistence

**Non-Goals:**
- Investigating or fixing the root cause of why `write_cluster_cache` might be called multiple times with different `saw_system_audio` values (that's a separate issue)
- Migrating or repairing existing corrupted data (not needed; only future writes are affected)

## Decisions

**Decision 1: Add `channel` to the DELETE WHERE clause**

Change the DELETE statement from:
```sql
DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ?
```
to:
```sql
DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ? AND channel = ?
```

**Rationale:** This is the minimal change that fixes the bug. The `channel` parameter is already passed to `write_cluster_cache`, so we just need to use it in the DELETE query.

**Alternatives considered:**
- Add a unique constraint on `(meeting_id, cluster_label, channel)` and use UPSERT instead of DELETE+INSERT. This would be more complex and require a migration. The current approach is simpler and achieves the same result.
- Change the function signature to require separate calls for each channel. This would be more invasive and break existing callers.

**Decision 2: No changes to `meeting_speakers` table**

The `meeting_speakers` table uses `ON CONFLICT(meeting_id, cluster_label)` for upserts, which is correct. Cluster labels are unique per meeting regardless of channel (e.g., `MIC_SPEAKER_00` vs `SPEAKER_00`), so there's no conflict.

## Risks / Trade-offs

**Risk:** If there are existing corrupted rows in the database (where `channel` doesn't match the cluster label's expected channel), they will remain corrupted.
→ **Mitigation:** This is acceptable. The bug only affects future writes. Existing data can be repaired manually if needed, but it's not critical for the system to function.

**Risk:** The fix assumes that cluster labels are unique per `(meeting_id, channel)` combination. If two different channels use the same cluster label (e.g., both use `SPEAKER_00`), they will be treated as separate caches.
→ **Mitigation:** This is the intended behavior. The code already uses different prefixes (`MIC_SPEAKER_*` for mic, `SPEAKER_*` for system in stereo mode) to avoid label collisions.
