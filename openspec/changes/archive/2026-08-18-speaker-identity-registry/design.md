# Design: speaker-identity-registry

## Context

Both diarization paths already compute ResNet34 INT8 embeddings (256-d) with the **same model** (`audio/diarization.rs::embed_segments` offline, `online_diarization.rs` Efficient mode per chunk) — and discard them. Speaker naming is per-meeting: `transcripts.speaker` holds a cluster label (`SPEAKER_00`, `MIC_SPEAKER_01`), `transcripts.speaker_label` holds a denormalized display name, and `meetings.speaker_names` is a JSON map of cluster label → name. `MeetingsRepository::update_speaker_label` propagates a rename by bulk-updating transcript rows within one meeting. Nothing persists across meetings.

Key code facts that constrain the design (verified against sources):

- polyvoice 0.17 is an **external crates.io dependency**. Its `SpeakerTurn` carries no embedding, and `ArrivalOrderSpeakerCache` exposes no embedding getters. Fast mode cannot harvest embeddings from the pipeline.
- **Efficient mode has no live labels** — clustering happens only at stop. There is nothing to rename mid-recording.
- **Fast mode emits live stable turns** — mid-recording rename is meaningful there.
- The online processor runs inside a spawned task (`ONLINE_DIARIZATION_TASK` static in `recording_commands.rs`); mid-session mutation needs a shared handle.
- **The meeting row is created at recording stop** (`TranscriptsRepository::save_transcript`), so live-session configuration (expected speakers) must travel in memory and be persisted at stop.
- Frontend already consumes `speaker_names` as a `{cluster_label: name}` map and renders via `SpeakerLabel` (inline text edit) in `VirtualizedTranscriptView.tsx`.

User decisions locked during exploration: (1) live mid-recording rename is **Fast-mode only**; (2) renames are **global**; (3) confident matches are **auto-assigned** (no suggestion chips); (4) embedding caches are **kept forever**, with consumed size surfaced in Settings; (5) each meeting may define an **expected-speaker allowlist** constraining auto-recognition candidates.

## Goals / Non-Goals

**Goals:**
- Global person registry with voiceprints; recognize known voices in future meetings automatically.
- Persist the embeddings diarization already computes (centroids + exemplars per cluster) instead of discarding them.
- Speaker editing: free text **and** dropdown of registry persons, on offline transcripts and live Fast-mode turns; editing **defaults to relabeling only the edited block** (per-transcript override), with an explicit "apply to all blocks of this speaker" option for cluster-wide naming; cluster-wide renames propagate to all blocks of the cluster, globally for linked persons.
- Live Fast mode: mid-recording rename enrolls prototypes and affects the remainder of the session; single-turn relabel applies to the edited turn live and to its transcripts at stop.
- Expected-speaker allowlist per meeting; empty list = match against all known speakers; allowlist never restricts manual assignment.
- Storage size of voiceprints visible in Settings.

**Non-Goals:**
- Registry management UI beyond storage stats (no merge/delete/dedup screens — manual SQL is acceptable until requested).
- Suggestion/confirmation UX for matches (auto-assign only).
- Incremental clustering for Efficient mode (no mid-recording labels there).
- Voiceprint cleanup/compaction tooling (caches kept forever; stats-only visibility).
- Embedding model upgrade / re-enrollment pipeline (the `model` tag only *guards*; no migration of prints across models).
- Changing the clustering algorithm, threshold calibration, or diarization quality itself.

## Decisions

### D1 — Split cluster label from person identity (three-table model)

```
speakers (id, name, is_me, timestamps)                      ← global person
speaker_embeddings (id, embedding BLOB, model, channel,     ← voiceprints
                    duration_secs, speaker_id NULL |
                    meeting_id+cluster_label NULL, created_at)
meeting_speakers (meeting_id, cluster_label,                ← per-meeting mapping
                  speaker_id NULL, centroid BLOB,
                  matched_by 'auto'|'user'|NULL, match_score)
meeting_expected_speakers (meeting_id, speaker_id)          ← recognition allowlist
```

`transcripts.speaker` keeps the cluster label (drives color/side; stable per meeting). Person identity lives only in `meeting_speakers`. Rename = single-row update; propagation is automatic via join. *Alternative considered*: denormalize `speaker_id` onto transcripts — rejected: bulk rewrites on reassignment, and renames would still need a join target.

### D2 — One embeddings table, two owners; enrollment = reparenting

A cluster's exemplar embeddings are written at diarization time with `meeting_id + cluster_label` set (cache). When the user links the cluster to a person, enrollment is:

```sql
UPDATE speaker_embeddings SET speaker_id = :person
WHERE meeting_id = :m AND cluster_label = :c
ORDER BY duration_secs DESC LIMIT :k;   -- best K exemplars
```

Recognition reads `WHERE speaker_id IS NOT NULL AND model = :current`. *Alternative considered*: separate prototype and cache tables — rejected: enrollment becomes copy+delete and the schema grows for no benefit. The `CHECK` constraint guarantees exactly one owner kind.

### D3 — Display names resolved by join at read time

Transcript queries LEFT JOIN `meeting_speakers`→`speakers` and return `COALESCE(speakers.name, transcripts.speaker_label)` as the display label. A per-transcript override (D10) takes precedence over the cluster mapping: `COALESCE(override_speaker.name, speakers.name, transcripts.speaker_label)`. `speaker_names` JSON and `speaker_label` become legacy fallback only; nothing writes them anymore. Global rename needs **no cache invalidation**. *Alternative considered*: write-through cache into `speaker_label` — rejected: global rename would need cross-meeting bulk updates; invalidation bugs are the classic failure mode.

### D4 — Brute-force cosine matching, no vector index

Prototypes are L2-normalized 256-d f32. Matching = max cosine over the candidate person's prototypes, best person wins, assigned if `score > τ` (τ = 0.7 constant in code; tune after field data). With ≤ a few hundred prototypes this is microseconds in Rust. Channel tag prefers same-channel prototypes when both kinds exist for a candidate set. *Alternative considered*: sqlite-vec / ANN index — rejected as massive overkill at this scale.

### D5 — Fast mode embeds chunks itself

Since polyvoice's `SpeakerTurn` exposes no embedding, Fast mode additionally runs the same `ResNet34Adapter` on each incoming chunk (exactly what Efficient mode already does) into a per-channel `EmbeddingBuffer`. Costs one extra INT8 inference per chunk. The pipeline still provides live speaker IDs; our buffer provides recognition input and enrollment data. At stop, buffered embeddings are grouped by pipeline speaker ID via time-overlap against stable turns (same `find_best_speaker` overlap logic already used for transcripts). *Alternative considered*: fork/patch polyvoice to expose cache embeddings — rejected: pins us to a fork for one getter.

### D6 — Live rename via shared `PrototypeStore`

```rust
type PrototypeStore = Arc<RwLock<PrototypeStoreInner>>;
// inner: speaker_id → Vec<prototype vecs>, session cluster→person bindings
```

Created at recording start (loaded with expected speakers' prototypes), shared between the processor task and a new `assign_live_speaker` Tauri command (static beside `ONLINE_DIARIZATION_TASK`; single active session is already guaranteed by `OnlineDiarizationGuard`). Each chunk embedding is matched against the store; hits relabel the outgoing turn live. Rename → bind cluster→person + merge that person's prototypes → subsequent chunks match. At stop, session cluster embeddings enroll (D2). *Alternative considered*: control channel (mpsc) into the processor task — rejected: RwLock is simpler and the critical section is tiny.

### D7 — Expected-speaker list: two delivery paths

- **Live**: frontend passes `expected_speaker_ids` with `start_recording*`; held in session state, loaded into the `PrototypeStore`, persisted to `meeting_expected_speakers` when the meeting row is created at stop.
- **Offline**: editable on the meeting page; written directly to the table. Because centroids are cached (D1), changing the list allows an instant re-match command (`rematch_meeting_speakers`) with **no audio re-processing**.

### D8 — Auto-assign only, `matched_by` provenance

Confident centroid/embedding matches set `meeting_speakers.speaker_id` with `matched_by='auto'` and the score. Any manual edit sets `matched_by='user'`; user bindings always win and are never overwritten by re-matching. Wrong auto-assignments are corrected by the same dropdown/text edit, which doubles as the enrollment signal.

### D9 — Model version guard, no cross-model migration

Each stored embedding carries `model = 'resnet34-int8'` (the polyvoice WeSpeaker ResNet34 INT8 both paths use). Matching filters to the current model tag. If the embedder is ever upgraded, old prints are simply ignored (clusters fall back to manual naming and re-enroll). *Rejected*: attempting similarity across models — embeddings from different models are not comparable.

### D10 — Per-transcript speaker override (single-block relabel)

The cluster→person mapping (`meeting_speakers`) is inherently per-cluster; a mis-identified single block of an otherwise correct cluster needs a **finer-grained** override. Add a nullable `transcripts.speaker_override_id` column pointing at `speakers.id`. The editor **defaults to single-block scope**: assigning a person (existing or newly created) writes only that transcript's `speaker_override_id` — no `meeting_speakers` change, no enrollment (a single block owns no cached embeddings). An explicit "apply to all blocks of this speaker" control performs the cluster-wide operation instead (`set_user_binding` + `enroll_cluster`, as in D8). Read-time resolution: `COALESCE(override_speaker.name, meeting_speakers→speakers.name, transcripts.speaker_label)`. Overrides participate in global rename via the join and are never touched by auto-recognition or rematch. In live Fast mode the same editor records a per-turn override (cluster label + turn time range → speaker_id) in session state; at stop-finalize the overridden turns are applied to their matched transcripts before assignments are emitted. *Alternative considered*: separate `transcript_speaker_overrides` table — rejected: a single nullable column is simpler and the existence check is a plain `IS NOT NULL`; the column stays null for the vast majority of transcripts.

### D11 — In-place, non-disruptive label updates

Speaker relabels must not disturb the transcript view: no full-list re-render, no scroll reset, no empty/loading flash. The editor's `onUpdate` callback applies the new name to **only the affected segment(s) in local React state** (`TranscriptContext` / paginated list state), keyed by transcript id; `VirtualizedTranscriptView` rows are keyed by segment id, so React reconciles just the changed row while the scroll container stays mounted — scroll offset and the rest of the list are untouched. Backend persistence runs first (single-block → `assign_block_speaker`, apply-to-all → `assign_speaker`); on error the local label is reverted and a toast shown, leaving the list otherwise unchanged. No full refetch of the meeting after an edit; background consistency with the DB is guaranteed by the read-time join (D3) on the next load. Live Fast mode uses the same path via `applyLiveSpeakerLabel` (per-turn or per-cluster). *Alternative considered*: refetch the meeting's transcripts after every assignment — rejected: remounts/rerenders the list, resets scroll, and can flash an empty/loading panel.

## Risks / Trade-offs

- **False-positive auto-assignments (wrong person labeled)** → τ=0.7 is conservative; channel-preferring match helps; user correction is one click and itself enrolls more prototypes, improving future matches. `matched_by='auto'` provenance allows auditing later.
- **Fast-mode extra embedding inference adds CPU during recording** → same INT8 inference Efficient mode already pays per chunk; bounded by VAD-filtered speech chunks only. Measure on the slowest supported machine; if hot, embed at half rate (every other chunk) with negligible recognition loss.
- **Mid-recording rename races with in-flight chunk processing** → RwLock write is short (bind + push prototypes); worst case one chunk gets the old label, which the DB save path reconciles via the final session binding map.
- **Same person on mic vs system channel produces different prints** → `channel` column recorded; matching prefers same-channel prototypes; over time enrollment naturally accumulates both kinds.
- **Storage grows unbounded (by design)** → 1 KB/embedding, capped K per enrollment; stats display gives the user visibility; cleanup tooling deferred until requested.
- **Single-block default may frustrate bulk renames** → the "apply to all blocks of this speaker" affordance is one click away in the same editor; defaulting to the narrow scope prevents accidental cluster-wide relabeling when only one block is wrong.
- **Per-block overrides can silently diverge from cluster identity over time** → display resolution always shows the override (user's last explicit choice); re-running diarization rewrites cluster labels but not overrides, so an override on a no-longer-existing block simply stops applying.
- **Legacy meetings have no embedding caches** → they show as unidentified clusters; manual naming works and enrolls if caches exist. A "learn from past meetings" backfill (re-embed from audio files) is a possible follow-up, not this change.

## Migration Plan

1. New migration adds the four tables + indexes, plus the nullable `transcripts.speaker_override_id` column (D10). Purely additive; existing `speaker_names`/`speaker_label` data untouched and still rendered as fallback.
2. No data backfill: historical meetings have no embeddings; nothing to migrate.
3. Rollback: drop the four tables; read path falls back to legacy columns automatically (COALESCE). Code revert removes writes.
4. No frontend feature flag; the UI degrades gracefully when the registry is empty (dropdown shows only "new name" affordance).

## Open Questions

- Exact τ for field audio (VoIP-compressed system channel especially) — start 0.7, adjust after real meetings; consider per-channel τ if system-channel matches prove noisier.
- Max exemplars K per enrollment (start: 8) and per-person prototype cap (start: 64, prune lowest `duration_secs`) — tune with storage stats.
- Whether `is_me` should drive an automatic mic-channel fast-path (skip matching, always label "Me") — deferred nicety, column exists.
