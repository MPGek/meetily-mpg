# Design

## Context

See `proposal.md` for the motivation. State of the code this change builds on:

- `run_offline_diarization` (`batch/orchestrator.rs`) diarizes the saved recording, rewrites the transcript rows' cluster labels (`split_rows_by_speaker`, `update_transcript_speaker`), then calls `persist_and_recognize_session`, which writes each cluster's centroid and exemplar cache and binds a cluster to a person when its **centroid** matches a prototype above the recognition threshold. Then it sets the status `complete`.
- `persist_and_recognize_session` is shared: the online stop-time path calls it too, and follows it with `recognize_transcript_rows`, which names each row from the embeddings overlapping that row (`per-row-speaker-recognition`). The offline path has no such step, and `the_batch_persistence_path_records_no_row_level_match` records that as a decision.
- Display resolution already prefers, in order: a per-block user override, a row-level automatic match, the cluster binding. A user-bound cluster (`matched_by = 'user'`) and an override are therefore never outranked, and nothing needs to be written to protect them.
- Enhance (`audio/retranscription.rs`) carries forward only user-confirmed identities and lets the offline analysis re-derive every automatic name, so the offline path decides the automatic names of an Enhanced meeting.
- What the offline pass persists is at most `MAX_CLUSTER_CACHE_EXEMPLARS = 32` segment embeddings per cluster label, each with its time window. A meeting recorded with live diarization also still holds that session's chunk embeddings: an offline pass replaces only the unassigned rows of the cluster labels *it* writes, so rows under the live session's labels stay. `refresh_transcript_row_matches` (what re-match runs) reads all of them.

## Goals / Non-Goals

**Goals:**

- After an offline pass, each row is named from the persisted embeddings that overlap it, so one cluster spanning two people, or one whose mean vector is diluted below the threshold, cannot decide the names of its rows.
- The existing matching rules apply unchanged; there is one definition of "recognized".

**Non-Goals:**

- Fixing the offline clustering that put two people into one cluster. Their rows now get the right names; the cluster label still holds both, as it does for a live session.
- Changing what re-match reads, or the exemplar cache cap.
- Naming rows from anything other than the persisted embeddings.
- Any change to the live path, the diarization output, DER, or the eval harness.

## Decisions

### D1: A separate step in the orchestrator, not a change to `persist_and_recognize_session`

The step runs in `run_offline_diarization` after `persist_and_recognize_session` (which writes the fresh exemplar cache the refresh reads) and before the status is set to `complete`. It is not folded into `persist_and_recognize_session`: that function is shared with the online stop path, which already follows it with its own row pass fed from different inputs, so extending it would name rows twice there and would give it two meanings.

- Alternative: call it from inside `persist_and_recognize_session`, so every caller gets it. Rejected for the reason above.

### D2: Run the existing refresh from the persisted embeddings; do not feed it the run's own

The step is a wrapper around `refresh_transcript_row_matches`: clear the meeting's row-level matches, then name each row from the persisted embeddings that overlap it. It reads the database, never the run's in-memory embeddings. The offline pass and a later re-match then run the same code over the same data and agree.

This replaces an earlier draft that fed `recognize_transcript_rows` the run's own `ClusteredEmbedding`s (all of them, so with better coverage than the capped cache). It was built, and measured, and dropped:

- **A faithful replay of the reported meeting (18-06) named 2 of 6 rows** from the run's 19 system embeddings, and one of them wrongly: the block that is Alex Shingel scored Greg 0.80 against Alex 0.75, and the Sviatlana and Marina blocks scored 0.57 and 0.58, under the 0.68 threshold. The run's embeddings are per-segment mean vectors; the registry's voiceprints are made of per-chunk embeddings, and the two do not score alike.
- **Like-for-like on the user's own assignments** (87 rows over 6 meetings with a user-set name and at least one cached embedding, leave-one-meeting-out, the two replays excluded; the user's name is the truth, so the sample leans towards rows the user corrected). Right / wrong / abstained, and right among the answered: cluster centroid (today) 25 / 54 / 8, 32 %; live chunk embeddings 32 / 19 / 26, 63 %; **all persisted embeddings (this design) 35 / 22 / 30, 61 %**; the offline run's own exemplars alone only cover 14 of the 87 rows (3 / 3 / 8).
- **A trap in the reported meeting itself.** 18-00 and 18-06 are the same audio played twice. Once the user assigned names in 18-00, the live chunk embeddings of 18-06 matched those voiceprints at 0.98, because they are the same audio. "Live got it all right, offline did not" therefore cannot be read off this pair, and the first version of this change's own check (5 of 6 rows named on the reported meeting) was flattered by it. The evidence above, which excludes the pair, is what the design rests on.

Coverage is the limit of this choice: the persisted embeddings overlap 85-100 % of a meeting's rows up to about 25 minutes, but only 38 % and 50 % on the two 44- and 51-minute meetings measured. Rows outside any stored embedding keep resolving through their cluster, exactly as today, so this is an improvement that never falls below the current behavior on coverage grounds.

- Alternative: feed the run's own embeddings (the earlier draft, above). Rejected on the replay and on the granularity mismatch.
- Alternative: union the persisted and the run's embeddings. Rejected: the run's embeddings add noise the measurements show, and it would make the offline pass disagree with a later re-match.

### D3: Clear the meeting's earlier row-level matches first, then record

A live session's stop-time pass, or an earlier offline run, may have left row-level matches. An offline run replaces the clusters and labels wholesale, so a leftover name would sit above the new result in the display order and the new run could never change that row. The refresh clears the meeting's row-level matches (`clear_meeting_auto_matches`, which leaves user overrides alone) before recording the new ones, so this comes with the reuse in D2 rather than as separate work.

One consequence to state plainly: an Enhance already drops the automatic names (only user-confirmed ones are carried), so after Enhance there is nothing stale; the case that matters is "Speakers" on a meeting whose live session named the rows.

### D4: The step cannot fail the diarization

The wrapper logs an error and swallows it, and the run still ends with the status `complete` and its normal result. The wrapper is a small function of its own so the failure path can be tested without a running app.

- Alternative: propagate the error like `persist_and_recognize_session` does. Rejected: row names are an improvement over the cluster names, not a precondition of the diarization; failing the whole run, after its rows were already rewritten, would be worse than the state the user has today.

### D5: Nothing is written to protect user decisions

Per-block overrides and user-bound clusters are protected by the display order, as for the live path, so the step never reads or writes them. It writes only `speaker_auto_id` and `speaker_auto_score`.

### D6: One wrapper, no new matching code

Everything that decides a name - the candidate prototypes and the expected-speaker allowlist, the threshold, the same-channel rule, the per-row selection - already lives in `recognize_transcript_rows` and is shared with the live path and with re-match. The change adds no matching code, so there is still one definition of "recognized".

## Risks / Trade-offs

- **It abstains more than the cluster centroid does, and is sometimes wrong.** On the like-for-like sample it answered 57 of 87 rows against 79 for the centroid, and 22 of its 57 were wrong (39 %) against 54 of 79 (68 %). A row it abstains on keeps the cluster name it has today. → Accepted: fewer wrong names is the point; recorded so the trade is visible.
- **The sample is small and leans towards corrected rows.** 87 rows, 6 meetings, user overrides as truth. → The direction is clear but the size of the gain is not; the design does not rely on the size.
- **Coverage on long meetings is partial** (38-50 % of rows on the 44- and 51-minute meetings measured), so most rows there still resolve through their cluster. → Unchanged from today for those rows; the cache cap is an open question.
- **The cluster label still holds two people** when the offline clustering merged them, so cluster-scoped actions (bind the cluster, "apply to all blocks of this speaker") still act on all of its rows. → Unchanged from the live path; only the automatic names improve.
- **A row's name can now differ from its cluster's binding**, so the speaker panel and a row can disagree. → That is the behavior `per-row-speaker-recognition` shipped for live sessions; the display order is what makes it coherent.
- **The reported meeting cannot demonstrate the gain** because it is a replay of audio the registry was taught (see D2). → The verification records that, and the evidence used instead.

## Migration Plan

No schema change and no data migration. Existing meetings are untouched until they are next diarized offline (the "Speakers" button, or an Enhance). Rollback is removing the one call in the orchestrator; the columns it writes already exist and are ignored when null.

## Open Questions

- Should the exemplar cache keep more than 32 embeddings per cluster, or the live chunk embeddings be kept on purpose rather than as leftovers, so that long meetings are covered better? It changes storage, not the behavior specified here, so it can be decided later.
