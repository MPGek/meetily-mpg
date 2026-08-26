## Why

Block-scope speaker assignments during live recording (the default "this block" correction) create enrolled voiceprint prototypes without `meeting_id` or `cluster_label` provenance. These prototypes appear in the Voiceprint Browser as "source unavailable" with no way to play the audio clip or navigate to the source meeting. Cluster-scope assignments ("apply to all") work correctly because they reparent existing cache rows that already carry provenance. The bug is in `enroll_embeddings_from_buffer`, which INSERTs new prototype rows but omits the provenance columns.

## What Changes

- Fix `SpeakerRepository::enroll_embeddings_from_buffer` to accept and persist `meeting_id` and `cluster_label` on newly inserted prototype rows
- Update the caller in `finalize_online_session` to pass `meeting_id` and `cluster_label` from each turn override into the enrollment function
- Existing prototypes created without provenance remain as-is (no migration needed — they are still functional for recognition, just lack playback/navigation in the browser)

## Capabilities

### New Capabilities

_None_

### Modified Capabilities

- `speaker-identity-registry`: Clarify that ground-truth enrollment from block-scope assignments MUST preserve `meeting_id` and `cluster_label` provenance on enrolled prototype rows, matching the behavior already established by cluster-scope enrollment via `enroll_cluster`
- `speaker-correction`: Clarify that inline block corrections (both live and offline single-block) MUST produce enrolled prototypes that retain full provenance (meeting_id, cluster_label, audio timecodes) so they are playable and navigable in the Voiceprint Browser

## Impact

- **Rust backend**: `speaker.rs` (enroll_embeddings_from_buffer signature + INSERT query), `recording_commands.rs` (finalize_online_session caller)
- **Database**: No schema changes — the `speaker_embeddings` table already supports optional `meeting_id`/`cluster_label` on prototypes (relaxed CHECK in migration 20260819000000)
- **Frontend**: No changes — VoiceprintBrowser already handles provenance display and playback correctly when `meeting_id` is present
- **Backward compatibility**: Fully backward compatible — existing rows without provenance are unaffected; new rows will carry provenance going forward
