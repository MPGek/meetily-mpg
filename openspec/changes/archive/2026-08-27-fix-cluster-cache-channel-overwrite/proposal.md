## Why

The `write_cluster_cache` function in `speaker.rs` deletes all `speaker_embeddings` rows for a given `(meeting_id, cluster_label)` pair before inserting new ones, regardless of the `channel` value. When online diarization runs with `saw_system_audio=true`, it creates both `MIC_SPEAKER_*` (channel='mic') and `SPEAKER_*` (channel='system') clusters. If the function is called again with `saw_system_audio=false` (e.g., due to a race condition or re-processing), the `SPEAKER_*` rows get overwritten with `channel='mic'`, corrupting the channel provenance required for correct speaker recognition and enrollment seeding.

## What Changes

- Modify the DELETE statement in `write_cluster_cache` to include a `channel` filter: `DELETE FROM speaker_embeddings WHERE meeting_id = ? AND cluster_label = ? AND channel = ?`
- This ensures that cluster caches for different channels are independent and do not overwrite each other
- The `meeting_speakers` table already uses `ON CONFLICT` on `(meeting_id, cluster_label)`, which is correct since cluster labels are unique per meeting regardless of channel

## Capabilities

### New Capabilities

None. This is a bug fix to existing behavior.

### Modified Capabilities

- `speaker-identity-registry`: The voiceprint storage requirement needs clarification that cluster cache writes are channel-scoped. When persisting exemplar embeddings for a cluster, the system SHALL only delete and replace rows matching the same channel, preserving cross-channel cache independence.

## Impact

- **Code**: `frontend/src-tauri/src/database/repositories/speaker.rs` — `write_cluster_cache` function (line ~284)
- **Data integrity**: Fixes corrupted `channel` values in `speaker_embeddings` table for meetings affected by the double-write bug
- **No API changes**: The function signature remains the same; only the internal DELETE query changes
- **No migration needed**: Existing data is not structurally affected; only future writes are corrected
