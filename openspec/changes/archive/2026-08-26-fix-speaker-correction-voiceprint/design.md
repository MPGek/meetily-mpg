## Context

After retranscription (enhance) and diarization, some transcript segments have `speaker = NULL` when diarization cannot match them to any detected speech turn (no overlap and no gap-fill). When users correct these blocks by assigning a speaker, the current implementation silently skips voiceprint enrollment because it cannot resolve the cluster label from the transcript row.

The enrollment path in `assign_block_speaker` calls `get_transcript_cluster` which returns `(meeting_id, speaker)` from the transcript. When `speaker` is NULL, the pattern match fails and `enroll_cluster` is never called.

## Goals / Non-Goals

**Goals:**
- Enable voiceprint enrollment on speaker corrections even when the transcript has no cluster label
- Resolve the appropriate cluster by matching the block's time range against stored cluster exemplars
- Maintain existing behavior when the transcript has a valid cluster label
- Provide clear feedback when enrollment produces zero voiceprints

**Non-Goals:**
- Changing diarization to always assign speaker labels (some segments legitimately have no match)
- Modifying the cluster-wide correction path (it already works correctly)
- Adding UI controls for manual cluster resolution

## Decisions

### Decision 1: Resolve cluster by time-overlap matching against cached exemplars

When a transcript has no cluster label, query `speaker_embeddings` for unassigned cache rows (`speaker_id IS NULL`) whose time windows overlap the transcript's `audio_start_time` to `audio_end_time` range. Group results by `cluster_label` and select the cluster with the longest total overlap duration.

**Rationale:** This approach uses existing data (cached exemplars already have time ranges) and requires no new queries against cluster centroids. It directly finds the embeddings that would be enrolled, making the resolution accurate.

**Alternatives considered:**
- Match against cluster centroids only: Less accurate because centroids don't have time ranges; would require iterating all exemplars anyway
- Use the meeting's expected speakers to narrow candidates: Adds complexity without clear benefit; time-overlap is sufficient
- Require user to manually select a cluster: Poor UX; the system can resolve this automatically

### Decision 2: Add a new repository method for time-based cluster resolution

Create `SpeakerRepository::resolve_cluster_by_time_overlap(pool, meeting_id, channel, time_range) -> Option<String>` that returns the best-matching cluster label. This method encapsulates the query logic and keeps the command layer clean.

**Rationale:** Separation of concerns; the repository handles data access, the command handles orchestration. The method is testable in isolation.

### Decision 3: Log a warning when enrollment produces zero voiceprints

When `enroll_cluster` returns 0 (no exemplars reparented), log a warning with the meeting ID and cluster label. This provides observability without breaking the user flow.

**Rationale:** Users need to know when their correction didn't create voiceprints, but a toast notification would be intrusive for an edge case. Logging is sufficient for debugging and future UX improvements.

**Alternatives considered:**
- Show a toast notification: Too intrusive for a rare edge case
- Return an error to the user: The label mapping is still valid; enrollment failure shouldn't block the correction

### Decision 4: Channel-aware resolution

When resolving the cluster, filter exemplars by the transcript's channel (mic or system) to maintain channel separation. Determine the channel from the transcript's `source_device` column: "System" → system channel, otherwise → mic channel.

**Rationale:** Consistent with existing enrollment behavior that keeps channels separate. Prevents cross-channel contamination of voiceprints.

## Risks / Trade-offs

**[Risk] Time-overlap matching may resolve to the wrong cluster when multiple clusters overlap the same time range** → Mitigation: Pick the cluster with the longest total overlap duration, which is the most likely correct match. In practice, clusters rarely overlap significantly in time because diarization assigns distinct time ranges to distinct speakers.

**[Risk] Performance impact from additional query on every single-block correction** → Mitigation: The query is indexed on `(meeting_id, speaker_id, audio_start_time, audio_end_time)` and only runs when the transcript has no cluster label (a rare case). The query returns at most a few rows and groups them in memory.

**[Risk] Existing voiceprints from the same meeting may be reparented multiple times if the user corrects multiple blocks** → Mitigation: `enroll_cluster` reparents the best-K exemplars; subsequent calls for the same cluster are no-ops because the exemplars are already reparented. The per-person cap (64) prevents unbounded growth.

**[Trade-off] Silent enrollment when cluster resolution fails** → The correction still applies the label mapping, which is the primary user intent. Enrollment is a secondary benefit. This matches the existing "correction with no audio still labels" scenario.
