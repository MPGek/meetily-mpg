## Context

See `proposal.md` for motivation. The relevant current state:

- `speaker_embeddings` enforces ownership with a `CHECK` from `migrations/20260819000000_add_voiceprint_provenance.sql`: either `speaker_id IS NOT NULL` (enrolled prototype) or `speaker_id IS NULL AND meeting_id IS NOT NULL AND cluster_label IS NOT NULL` (unassigned cache). The two kinds are therefore disjoint and exhaustive.
- `SpeakerRepository::clear_all_voiceprints` (`frontend/src-tauri/src/database/repositories/speaker.rs:1690`) already deletes both kinds and returns `ClearAllResult`; its command (`frontend/src-tauri/src/database/speaker_commands.rs:552`) additionally clears the in-memory `PrototypeStore` (`prototypes`, `bindings`, `session_embeddings`).
- `SpeakerRepository::enroll_cluster` (`speaker.rs:541`) reparents the best-K cache rows of a `(meeting_id, cluster_label)` into a speaker, ordered by `duration_secs DESC`, with no `model`/`channel`/provenance filter. This is the only path by which cache rows influence recognition.
- `load_prototypes` (`speaker.rs:833`) reads only `speaker_id IS NOT NULL AND model = <active tag>`, so unassigned caches never participate in recognition directly.
- The Settings browser (`frontend/src/components/VoiceprintBrowser.tsx`) already renders the Storage card with `Prototypes`/`Unconfirmed caches` counts, the `Remove all` button, and a `ConfirmClearAllDialog`; the typed wrapper lives at `frontend/src/services/recordingService.ts:392`.

## Goals / Non-Goals

**Goals:**

- Remove the entire unassigned cache layer without touching enrolled prototypes, the registry, cluster bindings, centroids, allowlists, or transcript overrides.
- Report what was actually deleted, including reclaimed embedding and stored-clip bytes, so the UI can show the impact before and after.
- Keep the operation a single, atomic, index-friendly statement on the existing schema.

**Non-Goals:**

- No schema migration and no change to the `speaker_embeddings` shape.
- No selective/paged purging (per meeting, per age, per model) in this change.
- No change to diarization, recognition thresholds, `load_prototypes`, or `enroll_cluster` filtering. Re-diarization remains the only way to regenerate caches.
- No change to the existing per-row cache rejection or to `clear_all_voiceprints`.

## Decisions

### D1: Predicate is ownership, not review state

Delete `WHERE speaker_id IS NULL`. `is_verified` (the "unverified" axis in the browser) is a review flag and is deliberately not used: a cache row that an automatic pass marked verified is still an unconfirmed cache, and the user's goal is to clear the whole legacy substrate.

Alternatives considered: filtering to `model <> 'titanet_large'` (rejected — titanet-era caches written before per-person splitting are structurally indistinguishable from fresh ones and are equally harmful as enrollment seeds), filtering to rows lacking provenance (rejected — same reason), and per-meeting selection (rejected as out of scope for the first cut; noted as an open question).

### D2: A dedicated repository operation and command, not a flag on `clear_all_voiceprints`

Add `SpeakerRepository::purge_unconfirmed_caches` and a `purge_unconfirmed_caches` Tauri command returning `{ deleted_caches, deleted_embedding_bytes, deleted_clip_count, deleted_clip_bytes }`.

Alternatives considered: adding an `only_caches: bool` parameter to `clear_all_voiceprints`. Rejected because the two operations differ in result shape (`ClearAllResult` carries `deleted_prototypes`, which is meaningless here) and in their in-memory consequences (D4); a shared command would make each caller responsible for remembering which fields matter.

### D3: Read counts, then delete, inside one transaction

Compute `COUNT(*)`, `SUM(LENGTH(embedding))`, `COUNT(audio_blob IS NOT NULL)`, and `SUM(LENGTH(audio_blob))` for the cache predicate and run the `DELETE` inside a single transaction. This guarantees the reported totals describe exactly the rows removed, matching the pre-read style of `clear_all_voiceprints` and `storage_stats`.

Alternative considered: `DELETE` first and report `rows_affected` only. Rejected because the confirmation and the post-action summary are meant to show reclaimed space, and post-delete byte totals are unrecoverable.

### D4: No `PrototypeStore` mutation, unlike `clear_all_voiceprints`

Cache rows are not loaded into the store: `PrototypeStore::load_with_model` populates `prototypes` from `load_prototypes` (prototypes only) and leaves `session_embeddings` empty. Live enrollment on rename goes through `rebind_cluster` (DB) and `enroll_embeddings_from_buffer` (`session_embeddings`, in-memory), neither of which reads DB cache rows other than via `enroll_cluster`.

Consequently a purge leaves loaded prototypes valid, and clearing the store would be actively wrong: it would discard in-session user bindings (`store.bindings`) and force a reload with no benefit. The spec's "live session sees the post-purge state" scenario is satisfied naturally — a live rename after a purge simply enrolls nothing, which is the documented no-cache behavior already covered by the existing test at `speaker.rs:1984`.

### D5: Control lives in the Storage card, next to `Remove all`

Add a second, visually distinct button in the Storage card header, disabled when `stats.cache_count === 0`, with a sibling confirmation dialog modelled on `ConfirmClearAllDialog`. The confirmation names the cache count, the affected stored clips, and the irreversibility.

Alternative considered: putting the bulk action in the "Unconfirmed caches (per meeting)" group header. Rejected because the counts and byte figures that justify the action live in the Storage card, and keeping the two removal actions adjacent makes their scope difference legible.

### D6: Leave the existing per-row rejection untouched

`reject_voiceprint(permanent: true)` remains the surgical tool for a single cache. The new control covers only the bulk case.

## Risks / Trade-offs

- **Irreversible loss of enrollment substrate for unnamed clusters** -> the confirmation states the cache count and irreversibility explicitly, and the spec records that re-diarization is the only regeneration path. Existing behavior already tolerates a cache-less cluster: `enroll_cluster` returns 0 without error (`speaker.rs:1984`).
- **Stored voice clips on cache rows are deleted too** -> the confirmation reports the affected clip count and bytes, and prototype-row clips are preserved. This is the main source of reclaimed space and is intentional.
- **A purge racing an active live session's finalize** -> a purge between cache persistence and cluster rebinding makes that rebind enroll nothing. Accepted: the operation lives in Settings and the semantics (enroll from post-purge state) are the same either way; the transaction prevents a partially applied purge.
- **Long `DELETE` on a large cache layer** -> the predicate is a single table scan with no matching index (`speaker_id` is indexed with `model`, but the NULL-owner case has no dedicated index). Acceptable for a desktop-scale SQLite store; if it ever matters, the delete can be chunked without changing the spec or the command contract.
- **Stats drift between the purge result and the browser summary** -> the browser reloads both `storage_stats` and `list_voiceprints` after the action, following the existing `handleClearAllConfirm` pattern.
- **Premise mismatch** -> caches do not affect clustering; they affect future enrollment. The UI copy must not promise a clustering fix, only cache cleanup.

## Migration Plan

No schema migration and no data backfill. The feature ships as a new command plus UI. Rollback is reverting the command registration and the UI control; already purged caches cannot be restored, which is the documented expected outcome.

## Open Questions

- Should a later change add scoped purging (per meeting, per source model, or per age) for users who want to keep fresh caches? Deferrable: it would add a filter to the same operation without changing the specs, approach, or tasks here.
- Should purging caches also drop the corresponding `meeting_speakers.centroid` values, so a re-diarization is forced rather than allowing a re-match against a centroid whose exemplars no longer exist? Deferrable: current behavior keeps re-match working, which is the safer default.
