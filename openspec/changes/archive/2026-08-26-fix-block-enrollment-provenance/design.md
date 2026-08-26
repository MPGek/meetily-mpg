## Context

See proposal.md — the bug is that `enroll_embeddings_from_buffer` INSERTs new prototype rows without `meeting_id` or `cluster_label`, while `enroll_cluster` (the cluster-scope path) correctly preserves provenance by reparenting existing cache rows. The schema already supports provenance on prototypes (migration 20260819000000 relaxed the CHECK constraint). The VoiceprintBrowser already renders provenance and enables playback when `meeting_id` is present.

## Goals / Non-Goals

**Goals:**
- Block-scope enrollment produces prototypes with full provenance (meeting_id, cluster_label, audio timecodes)
- Voiceprint Browser displays source meeting and enables playback for all newly enrolled prototypes
- No schema changes, no data migration, no frontend changes

**Non-Goals:**
- Backfilling provenance on existing prototypes created without it (they remain functional for recognition, just unplayable)
- Changing the Voiceprint Browser UI
- Adding a migration to retroactively fix old rows

## Decisions

### Decision 1: Extend `enroll_embeddings_from_buffer` signature with provenance parameters

Add `meeting_id: &str` and `cluster_label: &str` parameters to the function. Include them in the INSERT query alongside the existing `audio_start_time`/`audio_end_time`.

**Rationale**: This is the minimal change — the function already receives the embedding timecodes, and the caller has `meeting_id` and `cluster_label` available from the turn override tuple. No new queries, no new data sources.

**Alternative considered**: Setting provenance in a separate UPDATE after INSERT. Rejected — adds a round-trip and risks inconsistency if the UPDATE is skipped.

### Decision 2: Pass provenance from the turn override loop in `finalize_online_session`

The caller already iterates over `(cluster_label, start, end, speaker_id)` tuples. The `meeting_id` is available as a function parameter. Pass both into `enroll_embeddings_from_buffer`.

**Rationale**: The data is already in scope — no lookups needed.

## Risks / Trade-offs

- **[Existing prototypes remain unplayable]** → Acceptable. These rows still work for recognition. Users can reject and re-enroll if needed. A future migration could backfill if demand arises.
- **[No validation that meeting_id exists]** → The foreign key constraint on `speaker_embeddings.meeting_id` references `meetings(id) ON DELETE CASCADE`, so an invalid meeting_id would fail at INSERT time. This is sufficient — the caller always passes the meeting_id from the finalized session, which was just created.
