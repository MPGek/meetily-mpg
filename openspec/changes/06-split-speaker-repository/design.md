# Design

## Context

See `proposal.md` for motivation. Current state that shapes the approach:

- `frontend/src-tauri/src/database/repositories/speaker.rs` is 4664 lines: consts and result structs (1-141), `pub struct SpeakerRepository;` (142) and its single `impl SpeakerRepository` block (144-1764), then `#[cfg(test)] mod tests` (1766-4664, 68 tests per `cargo test -p meetily --lib speaker -- --list | grep -c ': test'`).
- `database/repositories/mod.rs` declares `pub mod speaker;` alongside five sibling repositories (`meeting`, `setting`, `summary`, `tags`, `transcript`, `transcript_chunk`); none of them are split into directories today, but Rust resolves `pub mod speaker;` to either `speaker.rs` or `speaker/mod.rs` identically, so converting to a directory module needs no change to `mod.rs` or `database/mod.rs`.
- `SpeakerRepository` is a unit struct (`pub struct SpeakerRepository;`, no fields) with every method as an inherent `async fn` taking `&SqlitePool` (or, for a few private helpers, `&mut SqliteConnection`) as its first argument — there is no `&self`. Every call site is `SpeakerRepository::method(pool, ...)`. Rust allows multiple `impl SpeakerRepository { ... }` blocks across multiple files in the same crate with no special syntax, so the split needs no trait or facade — just one file per responsibility, each opening its own `impl SpeakerRepository { ... }` block.
- Callers (verified with `grep -rn "SpeakerRepository" frontend/src-tauri/src | grep -v repositories/speaker`): `audio/diarization.rs` (10 call sites), `audio/online_diarization.rs` (2), `audio/recording_commands.rs` (8), `database/speaker_commands.rs` (whole-file import + ~30 call sites), plus a doc-comment reference in `audio/speaker_recognition.rs`. All go through `crate::database::repositories::speaker::{SpeakerRepository, ...}` — none reach into a specific sub-file, so as long as `mod.rs` re-exports every type at the same path, no caller changes.
- Inherent-method visibility in Rust is determined by the module the `fn` is lexically defined in, not by which file happens to contain the surrounding `impl` block textually. A private (`async fn`, no `pub`) helper defined inside the `enrollment` module is invisible from the `voiceprints` module even though both are children of `speaker`, unless it is raised to at least `pub(super)` (visible to the parent `speaker` module and, transitively, to `speaker`'s other children). Exactly one such helper exists: `enforce_prototype_cap` (149-173, private today), called from `enroll_cluster`/`enroll_block_window`/`enroll_embeddings_from_buffer` (destined for `enrollment.rs`) and from `reconfirm_voiceprint` (destined for `voiceprints.rs`). The other private helper, `demote_foreign_prototypes_conn` (700-749), is only called from `demote_foreign_prototypes` and `enroll_block_window`, both destined for `enrollment.rs`, so it stays private.
- `meeting_speakers` has a `UNIQUE(meeting_id, cluster_label)` constraint (evidenced by the `ON CONFLICT(meeting_id, cluster_label)` upsert in `write_cluster_cache`, line 313), so the `(meeting_id, cluster_label)` pairs collected by `replace_speaker`/`preview_replace_speaker` from `meeting_speakers` are never duplicated. This is what makes the N+1 fix's `GROUP BY` sum exactly equal the original per-pair loop's sum.

## Goals / Non-Goals

**Goals:**
- Each new file is independently readable: one responsibility, its own doc comments, its own tests.
- Zero caller-visible change: every existing `SpeakerRepository::method(...)` call and every `use ...::speaker::{Type, ...}` import keeps compiling unchanged.
- `cargo test -p meetily --lib speaker` stays green with the same 68 tests throughout the move (verified after every task, not just at the end).
- Fix the `replace_speaker`/`preview_replace_speaker` N+1 without changing either function's return value for any input.

**Non-Goals:**
- No change to the database schema, SQL result shapes, or any `#[derive(Serialize)]` wire type.
- No change to the other ~6 non-clone clippy warnings in this file (03's scope) or to any other file's clippy warnings.
- No change to `replace_speaker`'s post-commit re-match loop's behavior — only a documenting comment.
- No new tests beyond the one needed to pin the N+1 fix's equivalence; existing test coverage is preserved as-is (moved, not rewritten), except for the 12 mechanical clone-to-slice fixes.

## Decisions

### D1: Directory-module layout, struct + consts in `mod.rs`, per-responsibility `impl` files

`speaker.rs` becomes `speaker/mod.rs` plus seven sibling files. `mod.rs` keeps:
- the shared imports (`bytes_to_embedding`, `embedding_to_bytes`, `MeetingExpectedSpeaker`, `MeetingSpeaker`, `Speaker`, `SpeakerEmbedding`, `chrono::Utc`, `sqlx`, `uuid::Uuid`) needed by more than one submodule,
- the four constants (`SPEAKER_EMBEDDING_MODEL`, `SPEAKER_EMBEDDING_MODEL_LEGACY_DASH`, `ENROLLMENT_BEST_K`, `PER_PERSON_PROTOTYPE_CAP`) as `pub const`, since `enrollment.rs`, `merge.rs`, and tests in several files reference them,
- `pub struct SpeakerRepository;` (the single definition; every other file only adds `impl` blocks for it),
- `mod crud; mod enrollment; mod binding; mod voiceprints; mod merge; mod overrides; mod stats;` plus `#[cfg(test)] mod test_support;`,
- `pub use {crud::*, enrollment::*, binding::*, voiceprints::*, merge::*, overrides::*, stats::*};` so every result/row struct (`SpeakerStorageStats`, `PrototypeRow`, `ClusterCentroid`, `Exemplar`, `VoiceprintRow`, `SpeakerVoiceprints`, `MeetingVoiceprints`, `VoiceprintBrowser`, `RejectResult`, `ReplaceResult`, `ClearAllResult`, `PurgeUnconfirmedCachesResult`) is still reachable at `crate::database::repositories::speaker::TypeName`, matching every existing `use` statement (e.g. `database/speaker_commands.rs:3`).

Each submodule's result/row struct is co-located with the function(s) that construct it (e.g. `ReplaceResult` moves to `merge.rs` with `replace_speaker`/`preview_replace_speaker`; `SpeakerStorageStats` moves to `stats.rs` with `storage_stats`), rather than leaving all structs in `mod.rs`. Rationale: the struct and the function that fills it are the unit a reviewer needs together; `pub use` re-export from `mod.rs` erases any path difference for callers.

- Alternative considered: keep every struct in `mod.rs` and only move functions. Rejected — it would leave `mod.rs` almost as long as a "misc" file and separate each DTO from the one function that builds it, defeating the point of the split.
- Alternative considered: a facade trait (`trait SpeakerCrud`, `trait SpeakerEnrollment`, ...) implemented per file. Rejected — `SpeakerRepository` has no fields and every method already takes the pool explicitly, so a trait would add ceremony (imports, trait bounds at call sites) without changing behavior; plain multi-file inherent `impl` blocks are idiomatic Rust and need no caller-visible change at all.

### D2: Responsibility → file mapping

| File | Functions (line refs are from the pre-split file) | Tests |
|---|---|---|
| `crud.rs` | `list_speakers` (178), `get_speaker` (187), `find_or_create_by_name` (199), `find_by_name` (238), `rename_speaker` (249), `set_expected_speakers` (930), `get_expected_speakers` (954) | `find_or_create_is_idempotent_and_case_insensitive`, `rename_speaker_updates_name`, `expected_speakers_round_trip` |
| `enrollment.rs` | `enforce_prototype_cap` (149, → `pub(super)`), `write_cluster_cache` (275), `enroll_cluster` (551), `enroll_block_window` (614), `demote_foreign_prototypes` (676, → `pub(super)`), `demote_foreign_prototypes_conn` (700, stays private), `enroll_embeddings_from_buffer` (760), `load_prototypes` (843) | `write_cache_and_enroll_reparents_best_k`, `block_correction_enrolls_cluster_cache`, `block_correction_without_cache_is_noop`, `enroll_block_window_only_enrolls_overlapping_channel_rows_capped_at_k`, `demote_foreign_prototypes_keeps_override_pinned_rows`, `repeated_block_correction_converges_on_latest_speaker`, `mixed_cluster_block_correction_does_not_enroll_siblings`, `null_cluster_block_correction_enrolls_only_overlapping_rows`, `enrollment_enforces_per_person_cap`, `enroll_embeddings_from_buffer_takes_overlapping_best_n`, `enroll_embeddings_from_buffer_keeps_channels_clean`, `write_cluster_cache_preserves_enrolled_prototypes`, `centroid_round_trips_through_bytes` |
| `binding.rs` | `get_meeting_speakers` (379), `get_cluster_channel` (394), `set_user_binding` (412), `set_auto_binding_if_unbound` (437), `confirm_speaker_binding` (471), `get_cluster_centroids` (519), `rebind_cluster` (589) | `auto_binding_does_not_overwrite_user_binding`, `confirm_cluster_binding_clears_score_and_sets_user`, `confirm_single_block_sets_override`, `confirm_does_not_duplicate_prototypes`, `confirm_with_no_bound_speaker_returns_zero`, `cluster_rebind_demotes_previous_speakers_prototypes` |
| `voiceprints.rs` | `list_voiceprints` (1013), `reject_voiceprint` (1134), `reconfirm_voiceprint` (1214), `verify_voiceprint` (1238), `verify_speaker` (1253), `verify_meeting_caches` (1270), `get_voiceprint_audio` (1287), `clear_all_voiceprints` (1700), `purge_unconfirmed_caches` (1730) | `list_voiceprints_grouped_shapes`, `reject_demote_and_reconfirm_enforces_cap`, `voiceprint_clip_columns_default_to_legacy_without_audio_file`, `voiceprint_verify_flag_transitions`, `purge_unconfirmed_caches_deletes_only_caches`, `purge_unconfirmed_caches_preserves_bindings_and_overrides`, `purge_unconfirmed_caches_reports_reclaimed_storage`, `purge_unconfirmed_caches_on_empty_layer_is_a_noop`, `purge_unconfirmed_caches_keeps_enrollment_semantics`, `purge_unconfirmed_caches_keeps_prototypes_with_live_provenance` |
| `merge.rs` | `replace_speaker` (1305), `preview_replace_speaker` (1442) | `replace_preserves_user_binding_and_override_and_is_atomic`, plus the new `replace_speaker_transcript_count_matches_per_cluster_sum` (task 9.2) |
| `overrides.rs` | `set_transcript_override` (1479), `clear_transcript_override` (1495), `get_transcript_cluster` (1509), `get_transcript_time_info` (1525), `resolve_cluster_by_time_overlap` (1542), `get_transcript_display_name` (1599), `apply_turn_overrides` (1621), `apply_cluster_binding_overrides` (1654), `get_display_names` (1678) | `block_override_takes_precedence_over_cluster_mapping`, `block_override_survives_rematch`, `resolve_cluster_by_time_overlap_single_match`, `resolve_cluster_by_time_overlap_multiple_clusters`, `resolve_cluster_by_time_overlap_no_match`, `resolve_cluster_by_time_overlap_channel_filtering`, `resolve_cluster_by_time_overlap_ignores_enrolled_prototypes`, `assign_block_speaker_with_null_cluster_enrolls_via_time_overlap`, `assign_block_speaker_with_null_cluster_no_overlap_still_labels`, `assign_block_speaker_with_valid_cluster_uses_direct_path`, `cluster_binding_overrides_persist_user_identity_on_rows` |
| `stats.rs` | `storage_stats` (971) | `storage_stats_match_raw_sum`, `storage_stats_disambiguates_provenanced_prototype_and_cache`, `voiceprint_storage_stats_separates_audio_bytes` |
| `test_support.rs` (`#[cfg(test)]`) | — | shared fixtures only: `setup_pool`, `insert_meeting`, `emb`, `insert_transcript`, `insert_transcript_window`, `insert_prototype_with_clip`, `insert_cache_row` |

`resolve_cluster_by_time_overlap` (1542) is grouped with `overrides.rs` rather than a dedicated file: the fixed 8-file layout has no separate "time-overlap resolution" module, and every caller of this function (`get_transcript_time_info`-driven correction flow) lives in the per-transcript-override flow, so it is cohesive with `overrides.rs` rather than `binding.rs`.

`set_expected_speakers`/`get_expected_speakers` (930/954) are grouped with `crud.rs`: they are simple speaker/meeting metadata reads and writes (an allowlist, not a cluster binding or embedding), the closest existing category to "speaker CRUD." **Assumption to confirm with the repo owner**: if the team considers the expected-speaker allowlist closer to "binding" (it does gate auto-recognition), it can move to `binding.rs` instead — either placement is a private, callers-blind decision with no functional difference.

### D3: Visibility fix — `enforce_prototype_cap` becomes `pub(super)`

Change its signature line from `async fn enforce_prototype_cap(...)` to `pub(super) async fn enforce_prototype_cap(...)`, defined in `enrollment.rs`. `pub(super)` exposes it to `enrollment`'s parent (`speaker`) and, through that, to `speaker`'s other children — including `voiceprints.rs`'s `reconfirm_voiceprint`, which calls `Self::enforce_prototype_cap(&mut tx, speaker_id)`. No other function needs a visibility change: every other cross-file callee (`get_cluster_centroids`, `load_prototypes`, `get_meeting_speakers`, `set_auto_binding_if_unbound`, `demote_foreign_prototypes`, `enroll_cluster`, `rebind_cluster`, ...) is already `pub async fn`, and `pub` on an inherent method is crate-visible regardless of which file's `impl` block declares it.

- Alternative considered: `pub(crate)` instead of `pub(super)`. Either compiles; `pub(super)` is chosen because it is the minimum visibility that satisfies the one real cross-module caller and documents the intended scope (usable only within `speaker`'s own submodules, not the wider crate).

### D4: N+1 fix — single `GROUP BY` query over a `VALUES` list

Both `replace_speaker` (1327-1338) and `preview_replace_speaker` (1456-1466) currently do:
```rust
let mut affected_transcripts_count: i64 = 0;
for (mid, cluster) in &affected_clusters {
    let (cnt,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM transcripts WHERE meeting_id = ? AND speaker = ?",
    ).bind(mid).bind(cluster).fetch_one(&mut *tx /* or pool */).await?;
    affected_transcripts_count += cnt;
}
```
one round trip per affected cluster. Replace with one query that lists every `(meeting_id, cluster_label)` pair as a `VALUES` clause and groups the count:
```rust
let mut affected_transcripts_count: i64 = 0;
if !affected_clusters.is_empty() {
    let values = affected_clusters.iter().map(|_| "(?, ?)").collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT meeting_id, speaker, COUNT(*) AS cnt FROM transcripts \
         WHERE (meeting_id, speaker) IN (VALUES {}) GROUP BY meeting_id, speaker",
        values
    );
    let mut q = sqlx::query_as::<_, (String, String, i64)>(&sql);
    for (mid, cluster) in &affected_clusters {
        q = q.bind(mid).bind(cluster);
    }
    let rows = q.fetch_all(&mut *tx /* or pool */).await?;
    affected_transcripts_count = rows.iter().map(|(_, _, cnt)| cnt).sum();
}
```
- Why a `VALUES`-tuple `IN`, not two separate `meeting_id IN (...) AND speaker IN (...)` lists: the latter is a cross product — a cluster label that is affected in meeting A but coincidentally also exists (unaffected) in meeting B would be over-counted. The `(meeting_id, speaker) IN (VALUES ...)` form matches exact pairs, identical to the original per-pair loop, and SQLite supports row-value `IN (VALUES ...)`.
- Why `GROUP BY` (a per-pair breakdown) rather than one bare `COUNT(*)` with the same `WHERE`: it is the same single round trip either way (the sum happens in Rust either way), but the grouped form gives a debuggable intermediate (`rows`) if the count is ever wrong, and the change description ("single GROUP BY query") is what the audit flagged.
- Why this preserves behavior exactly: `meeting_speakers` has `UNIQUE(meeting_id, cluster_label)`, so `affected_clusters` is a distinct-pairs list; summing per-pair `COUNT(*)` (old) and summing the grouped `COUNT(*)` over the same distinct pairs (new) are the same value for any input, including zero affected clusters (both early-return `0`).
- Applies to both functions because they are the same query duplicated; fixing only `replace_speaker` and leaving `preview_replace_speaker`'s N+1 in place would leave the preview (used by the UI to show impact before committing) with the exact same latency problem the change is meant to fix.

### D5: Keep the post-commit re-match loop outside the transaction, documented

`replace_speaker`'s re-match loop (1387-1432) reads `get_cluster_centroids`, `load_prototypes`, `get_meeting_speakers` and writes `set_auto_binding_if_unbound` — all four take `&SqlitePool`, not a transaction handle, and the loop already treats every result as best-effort (`.unwrap_or_default()`, `let _ = ...`). Two options:

1. **Keep it outside the transaction (chosen).** Add a comment at line 1386 (right after `tx.commit().await?;`) stating: this step is intentionally outside the transaction and best-effort; a failure here does not undo the successful re-bind/delete above, and re-running `replace_speaker` (or the next scheduled diarization pass) will re-derive the same bindings from the now-committed centroids.
2. **Fold it into the transaction.** Would require changing `get_cluster_centroids`, `load_prototypes`, `get_meeting_speakers`, and `set_auto_binding_if_unbound` to accept a generic executor (`impl SqliteExecutor` or an explicit `&mut SqliteConnection`) so they can run against `tx` instead of `pool` — a signature change touching every other caller of those four functions (`audio/diarization.rs`, `audio/online_diarization.rs`, `audio/recording_commands.rs`), and it would change behavior: a re-match failure (e.g. a transient lock) would now roll back the otherwise-successful cluster re-bind and prototype deletion, turning a "nice-to-have downstream refresh" into a hard dependency.

Option 1 is chosen: it is a documentation-only change (matches `skip_specs: true`), and Option 2's blast radius (three other files' function signatures) and behavior change are out of proportion to the problem — the loop is deliberately best-effort today, and that is the correct semantics for a downstream convenience, not a defect.

### D6: Mechanical clone-to-slice clippy fixes travel with their tests

`cargo clippy -p meetily --all-targets --message-format=short` reports exactly 12 "unnecessary use of `clone` to create a slice from a reference" warnings in this file today (lines 1884, 1922, 1985, 2586, 2633, 2682, 3370, 3391, 3873, 4012, 4061, 4093 — all inside test bodies, all of the shape `&[speaker.id.clone()]` used where a one-element slice of a reference is needed). Each becomes `std::slice::from_ref(&speaker.id)` (clippy's own suggested fix) as the containing test moves to its new file in tasks 3-8; no separate cleanup pass is needed. **Assumption to confirm**: if `03-lint-baseline-cleanup` (which applies before this change, per the ordered plan) already resolves this clippy category repository-wide, these 12 sites will already be fixed by the time this change is applied, and the corresponding task step becomes a no-op verification (clippy count is already 0) rather than an edit.

## Risks / Trade-offs

- **Intermediate broken-compile states while moving code across 8 files** → Mitigated by sequencing tasks as one module extraction at a time (never all at once), each ending in a green `cargo check -p meetily` and unchanged test count, so a bad task is caught immediately and is easy to `git diff`/revert in isolation.
- **`pub use` re-export glob (`pub use crud::*;` etc.) could silently pull in an unintended public item if a submodule later adds one** → Accepted: this is the same trade-off any `mod.rs`-with-re-exports pattern makes; the alternative (naming every re-exported item) is verbose for 12+ types and the existing sibling repositories (`meeting.rs`, `transcript.rs`) do not need this pattern today because they are not split.
- **The N+1 fix changes the exact SQL text**, so any test asserting on query count/log content (none currently do) would need updating → No such test exists today; task 9.2 adds a new one asserting the *result* is unchanged, not the query shape.
- **Test count drift going undetected** → Mitigated by recording the baseline (68) up front and re-checking `cargo test -p meetily --lib speaker -- --list | grep -c ': test'` after every task, not only at the end.

## Migration Plan

Pure refactor, applied as an ordered sequence of moves (see tasks.md); no data migration, no IPC change, no deployment step beyond the normal build. Each task is independently revertible (`git revert` of a single task's commit) because each leaves the crate compiling and the test suite green.

## Open Questions

- Confirm the `set_expected_speakers`/`get_expected_speakers` → `crud.rs` placement (D2) versus `binding.rs`; either compiles and is caller-invisible, but the team may have a preference for future additions to that area.
- Confirm whether `03-lint-baseline-cleanup` already covers the 12 clone-to-slice warnings (D6); if so, task 3-8's clippy sub-steps become verification-only.
