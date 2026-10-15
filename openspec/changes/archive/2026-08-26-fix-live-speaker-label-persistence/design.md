## Context

See `proposal.md` — Why for the motivation. Current state (from codebase exploration):

- Fast-mode live turns are labeled per-chunk: the mic prefix is chosen from `self.saw_system_audio` at the moment each chunk is processed (`audio/online_diarization.rs:525`), so early mic chunks emit `SPEAKER_NN` and later ones `MIC_SPEAKER_NN`. Stop-time `finalize()` relabels all mic transcripts `MIC_SPEAKER_NN` (`online_diarization.rs:694-700`), so labels the user clicked live can never match the persisted labels.
- `PrototypeStore` keys session embeddings by bare pipeline speaker index (`session_embeddings: HashMap<usize, …>`) while the mic and system pipelines both number speakers from 0, so a rename's enrollment seeds can mix both channels (`online_diarization.rs:126, 195-211`).
- The frontend overwrites every transcript's `speaker` with the raw stop-time assignment (`hooks/useRecordingStop.ts:269-277`) and never writes the user's chosen name; the rename survives only through the `meeting_speakers` join (`database/repositories/meeting.rs:13`), keyed by the exact persisted label.
- The live view re-matches all transcripts against the cumulative turn list on every turn and lets a turn's stale `display_name` override a user-set `speaker_label` (`contexts/TranscriptContext.tsx:126-142`) — no pinning.
- The display-name select already joins `meeting_speakers`; `matched_by` and `match_score` already exist on the row, so provenance/confidence need only be surfaced, not stored.

## Goals / Non-Goals

**Goals:**
- Make live-displayed labels and stop-time persisted labels identical by construction, so user renames survive recording stop.
- Treat user-selected speakers as ground truth: enroll the covering embeddings into the person's global prototypes.
- Surface auto-vs-user provenance and confidence in every speaker label renderer, with `(auto)` + score only for auto matches.
- Stop the live-view revert of user-pinned labels.

**Non-Goals:**
- Changing the label scheme itself (`MIC_SPEAKER_NN` / `SPEAKER_NN`) or the recognition threshold τ=0.7.
- Live label emission for Efficient mode (remains batch; persistence changes still apply via `finalize()`).
- Schema migrations (no new columns/tables needed; everything reuses existing rows).

## Decisions

### Decision 1: Session-stable mic prefix decided at recording start

Pass whether a system device was selected (`has_system_device`) into `OnlineDiarizationProcessor::new` from `start_recording` (device resolution already happens before the processor is spawned, `recording_commands.rs:179-445`). Compute once:

```rust
mic_prefix = if has_system_device { "MIC_SPEAKER" } else { "SPEAKER" }
```

Use this single value for **all** live mic turn emission (`process_chunk`) and for the mic side of `finalize()`. Keep `saw_system_audio` only to decide whether system segments exist to match (mono-materialized sessions), never to select the mic prefix.

- **Why:** the flapping prefix is the primary cause of "mic renames never save" — the bound label can never match the persisted label. Fixing it at the source removes the entire class of mismatch rather than papering over it at each join.
- **Alternative considered:** snapshot the prefix at first system chunk and re-label buffered turns; rejected because it requires retroactive re-emission and does not guarantee stability for the first chunks. Storing the prefix per buffered turn and reusing it at finalize was considered but yields split prefixes within one mic channel, violating the "channel-stable" spec.

### Decision 2: Key session identity state by (channel, pipeline_id)

Change `PrototypeStore`:

- `session_embeddings: HashMap<(String channel, usize pipeline_id), Vec<(Vec<f32>, f32)>>` (duration kept; channel moves from the tuple into the key).
- `push_session` uses the channel already passed to it (`online_diarization.rs:574-583`).
- `bind(cluster_label, speaker_id, name)` resolves the channel from the label prefix (`MIC_SPEAKER_` → mic; `SPEAKER_` → system when stereo, mic when mono) and the id from `parse_pipeline_id`, then seeds prototypes from only that channel's embeddings — never both.

`assign_live_speaker` keeps its signature (label-based); the channel resolution lives in one place (`PrototypeStore`) so callers don't change.

- **Why:** the current `HashMap<usize, …>` silently merges mic-id-1 and system-id-1 embeddings, contaminating enrollment/recognition after a rename. With Decision 1, label prefixes are unambiguous within a session, so label→(channel, id) resolution is exact.
- **Alternative considered:** passing an explicit `channel` arg through `assign_live_speaker`; rejected as redundant once prefixes are stable, and it would require source-device threading through the speaker editor.

### Decision 3: Stop-time assignments already carry the user's identity

With Decisions 1+2, `finalize()` needs no new matching logic: the assignments it emits use the same label the user clicked, and `finalize_online_session`'s existing `set_user_binding` + auto-recognition guard (`set_auto_binding_if_unbound` preserves `matched_by='user'`, `speaker.rs:268-293`) will bind the right rows. No change to the frontend's stop-time overwrite is required for label consistency.

Additionally, `finalize()` SHALL return the raw per-channel timestamped embedding buffers (from `mic_emb`/`sys_emb`) in addition to the grouped `OnlineClusterEmbeddings`, so ground-truth block enrollment (Decision 4) has the full time-indexed set. `OnlineSessionData` gains these buffers.

- **Why:** the reconciliation the specs require is achieved by making labels identical end-to-end rather than by an extra label-remapping pass that risks new mismatches.
- **Alternative considered:** remapping raw assignments to user labels in `finalize()`; rejected because `transcript.speaker` is contractually a cluster label — user identity belongs in `meeting_speakers`, which the label-prefix fix already makes reachable.

### Decision 4: Ground-truth enrollment from user-assigned blocks

Add to `SpeakerRepository`:

- `enroll_embeddings_from_buffer(pool, speaker_id, channel, embeddings: &[(f32, f32, Vec<f32>)], window: (f32, f32))` — collects embeddings whose window overlaps the block's `[start, end]`, inserts them as direct prototypes (`speaker_id` set, `meeting_id`/`cluster_label` NULL), best-N by duration, enforcing the existing per-person cap.
- Reuse the same cap/count logic as `enroll_cluster` (`speaker.rs:329-379`) — factor the cap enforcement into a shared helper.

Call sites:
- `finalize_online_session`: for each flattened turn override (`ONLINE_TURN_OVERRIDES`), look up the matched transcript's time window and call the new enrollment against the channel buffer carried in session data. This covers live single-block renames made with `assignLiveSpeakerBlock`.
- Cluster-wide live bindings already enroll via `enroll_cluster`; with Decision 2 their seed set is now channel-clean.

- **Why:** a user's explicit pick is stronger than any automatic match, so its audio should strengthen the person's identity. The per-block override previously skipped enrollment because "a single block owns no cached embeddings" — the session buffers now change that.
- **Alternative considered:** waiting for optional offline re-clustering to produce cache rows for the block; rejected — the user expects the choice to matter in later meetings immediately.

### Decision 5: Surface provenance + confidence through existing rows

- Extend `meeting.rs`'s `TRANSCRIPT_DISPLAY_SELECT` to also select `ms.matched_by AS speaker_matched_by`, `ms.match_score AS speaker_match_score`, and a provenance enum resolved server-side from the existing `COALESCE` precedence (override → user → auto → fallback). Return these alongside `speaker_label`.
- Extend the live `SpeakerTurn` payload with `matched_by` and `match_score` from the recognition result (`speaker_recognition::MatchResult` already carries the score; `online_diarization.rs:547-549` needs to retain it instead of just the name).
- Frontend: `SpeakerLabel` renders `name` + ` (auto)` + similarity % when provenance is auto; plain name for user/override; formatted cluster label unchanged for fallback.

- **Why:** `matched_by`/`match_score` already exist; surfacing them is display plumbing only. The `(auto)` marker makes auto-identities verifiable at a glance, and omitting it for user picks emphasizes ground truth.
- **Alternative considered:** deriving `(auto)` from `speaker_label` string matching; rejected as fragile (a user could legitimately name someone "Alice (auto)").

### Decision 6: Pin user labels on the live view

In `TranscriptContext`, keep `pinnedRef: Map<transcriptId, { cluster, name }>`. `applyLiveSpeakerLabel` records the pin (block-scope pins that transcript; cluster-scope pins every transcript currently carrying the cluster label). The `onSpeakerTurn` rematch skips any transcript whose pinned entry matches the turn's cluster — the pinned name and cluster stay frozen until the user edits again. A turn for a different cluster only applies to unpinned transcripts.

- **Why:** stops the "changed for a moment, then reverted" loop caused by stale `display_name` on turns that stabilized after the user renamed. The pin is intentionally display-side; persistence correctness comes from Decisions 1-4.
- **Alternative considered:** suppressing turn events after a rename; rejected — turns also carry cluster changes for *other* transcripts, which must keep flowing.

## Risks / Trade-offs

- [Session config says stereo but the system device is silent/missing] → mic transcripts get `MIC_SPEAKER_NN` in a de-facto mono session. Mitigation: same assumption offline diarization already makes from the file's channel count; acceptable and consistent.
- [Direct prototype inserts bypass the cache-write path] → rows are owned by the speaker with no cluster provenance. Mitigation: identical row shape to enrolled prototypes; cap enforcement shared with `enroll_cluster`.
- [Pinning freezes a block's live cluster, so late re-clustering won't reflow it] → Behavior is intentional (user override is authoritative); the stop-time override reasserts it in the DB.
- [Larger `OnlineSessionData` (raw embeddings) held between stop and finalize] → Bounded by session speech length; buffers were already held in memory during recording, so this only extends their lifetime by the finalize window.
- [Frontend provenance fields touch many render paths] → Single `SpeakerLabel` component owns formatting; a shared helper formats `(name, provenance, score)`.

## Migration Plan

- No schema migration. No config or settings changes.
- Rollback: revert backend prefix/handling changes and frontend pinning; live behavior returns to current state. Provenance fields are additive and ignored by older renderers.

## Open Questions

- Should the `(auto)`/confidence marker also appear on the summary/meeting title when a speaker is named? Deferred — display surfaces beyond the transcript view are out of scope for this change.
- Should ground-truth enrollment prune against per-person duration quality, or is duration-order insertion (best-N) sufficient? Answered by reusing `enroll_cluster`'s existing best-duration-then-cap logic; no open question.