## Context

See proposal.md — the bug is in `write_cluster_cache` at `speaker.rs:284`. The DELETE query removes all rows for a cluster, including enrolled prototypes. The fix narrows the query to only delete cache rows.

## Goals / Non-Goals

**Goals:**
- Fix the DELETE query to preserve enrolled prototypes
- Add a regression test to prevent recurrence

**Non-Goals:**
- No schema changes
- No API changes
- No changes to enrollment logic

## Decisions

**One-line query fix**: Add `AND speaker_id IS NULL` to the DELETE in `write_cluster_cache`.

**Rationale**: The function's purpose is to refresh unassigned exemplar caches. Enrolled prototypes are owned by speakers, not clusters, and should never be deleted by cache refreshes. This is the minimal, correct fix.

**Alternative considered**: Separate the cache and prototype tables. Rejected — the current schema uses a single `speaker_embeddings` table with `speaker_id` as the discriminator. The fix aligns the query with the schema's intent.

## Risks / Trade-offs

**Risk**: None — the fix narrows the DELETE to its intended scope. No behavior changes for cache rows; only prevents accidental deletion of prototypes.

**Mitigation**: Regression test verifies prototypes survive a cache refresh.
