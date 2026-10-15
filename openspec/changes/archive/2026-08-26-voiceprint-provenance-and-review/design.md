## Context

See proposal.md (Why) and the capability specs for the required behavior.

Today `speaker_embeddings` is a "one table, two owners" store (migration `20260817000000_add_speaker_identity_registry.sql`):

- **Unassigned cache**: `speaker_id` NULL, `meeting_id` + `cluster_label` set (1179 rows today). Has `channel` and `duration_secs`, but **no timecodes**.
- **Enrolled prototype**: `speaker_id` set, `meeting_id`/`cluster_label` forced NULL by a CHECK constraint and **actively nulled on enrollment** (`enroll_cluster`, speaker.rs:371). The 56 confirmed prototypes have lost all origin.

Two facts make provenance recoverable for new data:

1. The **offline** diarization path already computes segment times — `RawSegment.time.start/.end` → `DiarizationSegment.start/.end` (diarization.rs:653-662) — but drops them when building `ClusteredEmbedding { speaker, embedding, duration_secs }` at diarization.rs:796-799 and 1143-1146 (which computes duration as `end - start`). Exactly two call sites.
2. The **online** path already keeps `(start, end, embedding)` triples in `mic_raw`/`sys_raw` (online_diarization.rs:123-125) and has start/end in scope at the two `ClusteredEmbedding` build sites (online_diarization.rs:854-857, 872-875).

Other anchors: `Exemplar` (speaker.rs:52), `write_cluster_cache` (speaker.rs:195), `enroll_embeddings_from_buffer` (speaker.rs:397), `storage_stats` (speaker.rs:545), `load_prototypes` (speaker.rs:451, filters `speaker_id IS NOT NULL AND model = ?`), meeting deletion manual cascades (meeting.rs:372-387). FK enforcement is **off** (meeting.rs comment "FK enforcement is off, so these are manual cascades").

## Goals / Non-Goals

**Goals:**
- Every embedding row (prototype or cache) record optional `meeting_id`/`cluster_label`/`audio_start_time`/`audio_end_time` provenance, populated going forward by both diarization paths and preserved across enrollment.
- A Settings voiceprint browser (speakers + unconfirmed caches) with per-row provenance and original-clip playback reusing the existing streaming player.
- Reject / reconfirm / whole-corpus replacement, with user bindings and block overrides preserved, running atomically.

**Non-Goals:**
- No fabrication of provenance for existing rows — no backfill of timecodes (not recoverable); the browser shows "source unavailable".
- No changes to recognition math, thresholds, or the prototype load query chained to `speaker_id` + `model`.
- No new playback engine; reuse `get_meeting_audio_path` + the existing HTML5/asset-protocol player.
- Not rebuilding clustering or segmentation.

## Decisions

### D1 — Schema: add columns + relax ownership CHECK via table rebuild
SQLite can `ALTER TABLE ADD COLUMN` for the two timecode columns, but cannot alter a CHECK. Because prototypes must now be allowed to also carry `meeting_id`/`cluster_label`, the two-owner CHECK must change, requiring the standard 12-step table rebuild.

New shape:
```sql
CREATE TABLE speaker_embeddings (
  id TEXT PRIMARY KEY,
  embedding BLOB NOT NULL,
  model TEXT NOT NULL,
  channel TEXT NOT NULL CHECK (channel IN ('mic','system')),
  duration_secs REAL NOT NULL DEFAULT 0,
  speaker_id TEXT,             -- owner: set => enrolled prototype
  meeting_id TEXT,             -- provenance (always set for unassigned caches)
  cluster_label TEXT,          -- provenance
  audio_start_time REAL,       -- NEW, seconds, relative to meeting audio timeline
  audio_end_time REAL,         -- NEW
  created_at TEXT NOT NULL,
  FOREIGN KEY (speaker_id) REFERENCES speakers(id) ON DELETE CASCADE,
  FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE,
  CHECK (
    (speaker_id IS NULL AND meeting_id IS NOT NULL AND cluster_label IS NOT NULL)
    OR (speaker_id IS NOT NULL)
  )
);
```
- The new CHECK: an **unassigned cache** must have meeting+cluster; an **enrolled prototype** may carry any provenance combination (including none). No row may have `meeting_id` set without `cluster_label` (or vice versa) except when owned by a speaker.
- During rebuild, copy existing rows as-is (legacy prototypes have NULL provenance, which satisfies the head `speaker_id IS NOT NULL` branch).
- FKs stay declared but RUNTIME FK enforcement stays off (decision above exists; don't flip it on as part of this change to avoid unrelated behavior change). Recreate the two existing indexes (`(speaker_id, model)`, `(meeting_id, cluster_label)`) and add `(meeting_id, cluster_label, speaker_id)` for the review listing.

**Alternatives considered:** keep the strict CHECK and store provenance in a separate sidecar table (`voiceprint_meta`). Rejected because it splits a row's identity across two tables, complicates the CHECK story anyway (the FK/provenance link), and adds a join to every prototype read; the rebuild is a one-time cost and the two-owner semantics are preserved in spirit.

### D2 — Thread start/end through both paths, drop reliance on duration math
- `ClusteredEmbedding` (diarization.rs:542) gains `start_secs`/`end_secs`; `Exemplar` (speaker.rs:52) gains the same. `duration_secs` stays as-is (used for best-K ordering and the prototype cap).
- Offline: at both build sites (diarization.rs:796, :1143) the source `DiarizationSegment.start/.end` are in scope — copy them onto `ClusteredEmbedding` instead of only `(end - start)`.
- Online: at both build sites (online_diarization.rs:854, :872) copy the `(start, end)` triple.
- `write_cluster_cache` inserts the new columns; `enroll_cluster` and `enroll_embeddings_from_buffer` carry them through unchanged.

Presence of `start_secs` implies `end_secs` (verified at write time); one nullable pair of columns, not two independent-null columns.

**Alternative considered:** keep timecodes only at cluster level (`meeting_speakers`) instead of per row. Rejected: the review UI plays a single embedding's clip, and reject/reconfirm operates per row; cluster-level ranges (union of transcript times) do not identify which segment produced a given exemplar.

### D3 — Enrollment preserves provenance instead of nulling
`enroll_cluster` (speaker.rs:370-377) currently does `UPDATE ... SET speaker_id = ?, meeting_id = NULL, cluster_label = NULL`. Change to set **only** `speaker_id`, leaving `meeting_id`/`cluster_label`/times intact. `enroll_embeddings_from_buffer` already inserts fresh rows — add the time-window fields it already knows (`e_start`, `e_end`) to the inserted columns. `load_prototypes` is untouched (already `speaker_id IS NOT NULL AND model = ?`), so recognition is identical.

### D4 — Meeting deletion removes only unassigned caches
`meeting.rs:376` deletes `speaker_embeddings WHERE meeting_id = ?`, which with provenance would delete prototypes that merely reference the meeting. Change to `WHERE meeting_id = ? AND speaker_id IS NULL` (delete unassigned caches; keep provenanced prototypes; the dangling `meeting_id` is a historical reference resolved with a LEFT JOIN to `meetings` that displays "deleted meeting" when absent).

### D5 — Rejection model: demote, don't destroy, by default
- **Reject prototype** = `UPDATE speaker_embeddings SET speaker_id = NULL WHERE id = ?` — the row naturally satisfies the unassigned-cache CHECK branch (it still has meeting+cluster+times), appearing in the browser's unconfirmed branch and remaining enrollable elsewhere. Optional hard delete per user choice.
- **Reject cache** = `DELETE`.
- **Reconfirm cache / reassign demoted row** = reuse `enroll_cluster`-style reparent (single row: set `speaker_id`, enforce cap) so cap/pruning behavior is shared.
- Rejection is safe because `PrototypeStore` is rebuilt on load and `load_prototypes` reads the DB at each recognition run.

### D6 — Whole-corpus replacement as an atomic re-map, not a re-cluster
Implement `replace_speaker(source, target | None)` in a single transaction:
1. Count & collect every `meeting_speakers` row with `speaker_id = source AND matched_by='auto'`.
2. For each: if `target` → `set_auto_binding_if_unbound(..., target)`, else unset `speaker_id`/score (leave matched_by 'auto' → re-match will repopulate or leave anonymous). User-bound rows are excluded.
3. Delete the source speaker's prototypes (`DELETE FROM speaker_embeddings WHERE speaker_id = source`) — this is the "reject" half; demoted variants (D5) are an alternative per-row path, not the bulk path.
4. Re-run the existing `rematch_meeting_speakers` logic over the affected meetings (centroids only; no audio).
5. Report affected meeting/cluster/transcript counts before commit; commit only on full success.
Per-transcript overrides are on `transcripts.speaker_override_id` and are never touched by this path, so they survive by construction.

### D7 — Browser data model and playback reuse
- One command, `list_voiceprints`, returns a grouped shape: speakers (`id`, `name`, `is_me`, `prototype_count`) + their `VoiceprintRow`s, plus an `unconfirmed` list grouped by meeting (`meeting_id`, `title`, `channel`, `audio_start/end`, `duration`, `cluster_label`). Filtered variants for speaker-only / unconfirmed-only as the tree expands (paginate by speaker to avoid loading 1179 bytes-heavy rows eagerly).
- Playback reuses `get_meeting_audio_path` (asset protocol + CSP already wired) and the frontend `useAudioPlayer`; add a `playRange(start, end)` that seeks to `start` and a `timeupdate` listener pauses at `end` (mirrors the existing play-from-utterance behavior).
- Row actions call new commands: `reject_voiceprint(id, delete: bool)`, `reconfirm_voiceprint(id, speaker_id)`, `replace_speaker(source, target|None, confirm)`.

### D8 — Storage stats disambiguated
`storage_stats` (speaker.rs:545) currently counts `cache = meeting_id IS NOT NULL`, which after D3 would double-count provenanced prototypes. Change: `prototype = speaker_id IS NOT NULL` (unchanged), `cache = speaker_id IS NULL AND meeting_id IS NOT NULL`. Registry count and total bytes unchanged. This is the source for the browser's summary.

### D9 — Browser collapse/expand and bulk toggle (UI state only)
Per-group collapsible state lives entirely in the frontend `VoiceprintBrowser` component; no backend or `list_voiceprints` shape change is required. Each speaker (`SpeakerVoiceprints`) and each meeting (`MeetingVoiceprints`) renders as an accordion/disclosure group with an accessible toggle (button with `aria-expanded`, keyboard Enter/Space, visible chevron). State is a `Set<string>` of expanded group ids (e.g. `speaker:${id}` / `meeting:${id}`) held in `useState`; default is all expanded so existing behavior is preserved and initial load shows content. Two bulk controls call `setExpanded(allIds)` and `setExpanded(empty)` respectively, acting on both branches at once. When a speaker has zero prototypes or the browser is empty, the header still renders and bulk actions are no-ops. No persistence beyond the view session is needed; lazy/paginated loading (D7) is unaffected because collapse only hides already-loaded rows. No new Tauri command is needed; playback/reject/reconfirm bindings are event-forwarded unchanged.

**Alternatives considered:** persisting collapsed state in `localStorage` or URL. Rejected for initial scope to avoid durable UX divergence and extra testing; can be added later without spec drift.

### D10 — Unified person-picker for reconfirm and replace (reuse, no duplication)
Reconfirm (`reconfirm_voiceprint(id, speaker_id)`) and whole-corpus replacement (`replace_speaker(source, target|None)`) both need a target-person choice. Instead of ad-hoc `window.prompt` / separate dialogs, the browser reuses the existing speaker assignment / rename picker component already used elsewhere in the app (the searchable person list with quick filter and create-new). The component is a controlled dialog that takes `excludeId?` (for replace, hide the source speaker), `allowAnonymous` (for replace, emit `null`), and `onSelect(id)` / `onCreate(name)` callbacks; it is fed by `list_speakers` (already present) and `find_or_create_by_name` semantics. Reconfirm shows the picker with anonymous disabled; replace shows it with anonymous enabled as an explicit row. After picking, replace proceeds to the existing affected-count confirmation (D6) and then calls `replace_speaker`. No new backend shape is needed — the picker only resolves an existing or newly created `speaker_id` (or `null`) before invoking the already-specified commands. Reuse keeps search behavior, keyboard handling, and creation validation identical across entry points and avoids a second divergent prompt path.

**Alternatives considered:** keep `window.prompt` for replace while reconfirm uses the rich picker. Rejected because it violates UX consistency asked for, duplicates validation, and hides create-new and anonymous affordances; the shared component exists and has no extra cost.

## Risks / Trade-offs

- **Table rebuild touches a live table** → run inside the existing SQLx migration framework with a transaction; keep the copy `SELECT` ordering on `id`; add an integration test asserting row counts and the legacy-NULL provenance survive the rebuild.
- **Timecode timeline mismatch** (diarization segment times vs. transcript `audio_start_time`/playback) → both derive from the meeting audio decode start; add a verification test on a real stereo/short meeting that `RawSegment` times align with transcript block times within a tolerance. If outliers appear, normalize times relative to the meeting audio start at write time.
- **Prototype rows referencing deleted meetings** accumulate dangling `meeting_id` → resolver LEFT JOIN shows "deleted meeting"; FK enforcement remains off so no cascade surprises; documented in code.
- **Bulk replacement blast radius** (re-binds many meetings) → atomically in one transaction with a pre-commit impact report (spec requires confirmation before executing); user bindings and overrides untouched.
- **Rejecting the last prototype of a speaker** silently removes them from recognition → the UI warns when the action empties the set and offers "replace across meetings" as the explicit alternative.
- **Heavy enumeration in the browser** → lazy-load per speaker / paginate; never ship all embeddings across IPC in one response.

## Migration Plan

1. New SQLx migration `20260819xxxx_add_voiceprint_provenance.sql`: ALTER-style rebuild of `speaker_embeddings` (add timecode columns, new CHECK, recreate indexes). Data-preserving; legacy rows keep NULL provenance.
2. Deploy Rust side changes in the same release as the migration (schema and writers are in one binary).
3. Rollback: migrations run forward only in this codebase (SQLx `migrate!`); the rebuild is additive (NULL columns), so no destructive step; if a regression appears, the previous build reads the new schema fine (extra columns ignored) and new writers are the only mutators to watch.

## Open Questions

- Whether rejected-but-kept ("demoted") voiceprints should count as unconfirmed caches in storage stats/counts for the summary line, or be flagged with an explicit "rejected" marker. Current design treats them as ordinary unconfirmed caches; a `rejected_at` column could be added later without schema rework of specs if the distinction proves useful.
- Exact UI grouping for speakers with zero prototypes after rejection (show as "no voiceprint" in the browser, or hide). Design defaults to showing them with an empty/state row so the user can reconfirm.
