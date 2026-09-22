# Proposal

## Why

A live Fast-mode recording names every emitted turn from that turn's own audio, so the labels a user watches during a meeting are mostly right. At recording stop the name comes from somewhere else: recognition runs once per cluster, over that cluster's centroid, and the winning name is stamped onto every transcript row of the cluster. When the live clustering has merged several people into one cluster — which it does — a single match renames most of the meeting.

Measured on a user's 2.7-minute session (`meeting-c39544ca-9eab-4834-a790-98fe389966c9`): cluster `SPEAKER_00` held 38 live turns spanning 6.8–161.5 s, its centroid matched "Greg" at 0.863, and all 18 of its rows — 77.1 s of the meeting's 128.9 s of transcript speech, 51% of rows — became Greg. The user saw correct names while recording and one name everywhere afterwards.

The evidence points at this step and away from the neighbouring suspects. Replaying the same 35 rows through the existing attribution rule assigned 34 by genuine time overlap and only 1 by the 30-second gap-fill, so attribution is faithful. Re-running the recording through the harness with 1 s, 3 s and 8 s chunks instead of the production recipe left the dominant label at 29–30% of turn time in every case, so chunk granularity is not the cause either. What remains is that a row's name is decided by its cluster rather than by its own audio.

The repair is available from data the app already persists. Each meeting caches per-cluster exemplar embeddings with their time spans (53 rows for this meeting), and matching each transcript row against the enrolled prototypes using the exemplars that overlap it, at the same recognition threshold, relabels **10 of the 18 wrong Greg rows** to Alex Shingel (6), Mikhail Shashalevich (3) and Siarhei Hryb (1), with 34 of 35 rows covered by an overlapping exemplar. The remaining rows either genuinely match Greg or fall below threshold and keep today's behaviour.

This is deliberately the small half of the problem. Splitting the merged cluster is `05b-live-diarization-accuracy`'s deferred stop-time re-clustering; this change stops one merged cluster from overwriting names that per-row evidence already contradicts.

## What Changes

- At recording stop, after the existing per-cluster recognition, the system also matches **each transcript row** against the candidate prototypes using the embeddings whose time window overlaps that row, and records the row's own match (speaker + score).
- A transcript row's displayed name gains one level of precedence: the user's per-block override still wins, then the row's own automatic match, then the cluster binding, then the legacy label, then the formatted cluster label. Provenance and score report whichever level won.
- A cluster the **user** bound keeps its authority: a row-level automatic match never overrides a user assignment, cluster-wide or per block.
- The re-match operation refreshes row-level matches from the cached embeddings and clears the ones it can no longer justify, so re-matching after an allowlist edit cannot leave stale row names outranking fresh cluster bindings.
- Cluster centroids, exemplar caches and enrollment keep working exactly as today; they remain the fallback for rows with no usable embedding.

Non-goals: splitting or re-clustering the merged cluster (owned by `05b-live-diarization-accuracy`), changing the time-overlap attribution rule, changing offline/batch diarization behaviour, and changing the live recording view, which already renders per-turn recognition.

## Capabilities

### New Capabilities

None. This modifies how existing recognition and display requirements behave.

### Modified Capabilities

- `speaker-identity-registry`: "Automatic speaker recognition" gains the per-row match alongside the per-cluster match; "Speaker display name resolution" gains the row-level level in its precedence chain and reports it as provenance; "Instant re-match on allowlist change" must also refresh row-level matches.
- `speaker-label-confidence`: "Automatic labels marked with provenance" must expose an automatic name and its score when the name came from a row-level match, not only from `meeting_speakers.matched_by`.

## Impact

- `frontend/src-tauri/src/audio/diarization/engine.rs` — `persist_session` gains the per-row pass, using the `(start, end, embedding)` buffers it already retains for the session.
- `frontend/src-tauri/src/audio/diarization/persist/clusters.rs` — per-row matching next to the existing per-cluster recognition.
- `frontend/src-tauri/src/audio/diarization/commands.rs` — `rematch_meeting_speakers` refreshes/clears row-level matches.
- `frontend/src-tauri/src/database/repositories/speaker.rs`, `.../meeting.rs` — write and read the row-level match; the display select's `COALESCE` and provenance `CASE` gain the new level.
- `frontend/src-tauri/migrations/` — one additive migration adding two nullable columns to `transcripts`. No existing column changes meaning; `transcripts.speaker_label` stays the manual label channel.
- No frontend change expected: the transcript queries already return `speaker_matched_by` and `speaker_match_score`, and the renderer already formats them.
