# Tasks

## 1. Baseline and scaffold

- [ ] 1.1 Record the pre-split test baseline: run `cargo test -p meetily --lib speaker -- --list | grep -c ": test"` and confirm it prints `68`; note this number in the PR/commit description as the invariant every later task re-checks; verify: command output is `68`.
- [ ] 1.2 `git mv frontend/src-tauri/src/database/repositories/speaker.rs frontend/src-tauri/src/database/repositories/speaker/mod.rs` (no content change in this step); verify: `cargo check -p meetily` succeeds and `cargo test -p meetily --lib speaker -- --list | grep -c ": test"` still prints `68`.

## 2. Shared test fixtures

- [ ] 2.1 Create `frontend/src-tauri/src/database/repositories/speaker/test_support.rs` with `#[cfg(test)] pub(super)` versions of the 7 shared fixtures currently inside `mod tests` in `mod.rs`: `setup_pool` (line 1771 pre-split), `insert_meeting` (1784), `emb` (1792), `insert_transcript_window` (2042), `insert_transcript` (3027), `insert_prototype_with_clip` (4118), `insert_cache_row` (4288). Declare `#[cfg(test)] mod test_support;` in `mod.rs`. Leave `mod.rs`'s existing `mod tests` block calling them via `use super::test_support::*;` for now (tests have not moved yet); verify: `cargo test -p meetily --lib speaker -- --list | grep -c ": test"` still prints `68` and `cargo test -p meetily --lib speaker` passes.

## 3. Extract `crud.rs`

- [ ] 3.1 Move `list_speakers`, `get_speaker`, `find_or_create_by_name`, `find_by_name`, `rename_speaker`, `set_expected_speakers`, `get_expected_speakers` into a new `impl SpeakerRepository` block in `speaker/crud.rs`, with their doc comments; declare `mod crud;` and `pub use crud::*;` in `mod.rs` (no structs to re-export from this file). Move `find_or_create_is_idempotent_and_case_insensitive`, `rename_speaker_updates_name`, `expected_speakers_round_trip` into a `#[cfg(test)] mod tests` block in `crud.rs` using `use super::*; use crate::database::repositories::speaker::test_support::*;`; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, and the test count is still `68`.

## 4. Extract `enrollment.rs`

- [ ] 4.1 Move `enforce_prototype_cap` (raise to `pub(super)`), `write_cluster_cache`, `enroll_cluster`, `enroll_block_window`, `demote_foreign_prototypes` (raise to `pub(super)`), `demote_foreign_prototypes_conn` (stays private), `enroll_embeddings_from_buffer`, `load_prototypes` into `speaker/enrollment.rs`, plus the `Exemplar` and `PrototypeRow` structs they use; declare `mod enrollment;` and `pub use enrollment::*;` in `mod.rs`. Move the 13 tests listed in design.md's `enrollment.rs` row into `#[cfg(test)] mod tests` in `enrollment.rs`; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `68`, and `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep "speaker/enrollment.rs" | grep -c "unnecessary use of \`clone\`"` is `0` (fix each occurrence with `std::slice::from_ref(&x.id)` while moving that test).

## 5. Extract `binding.rs`

- [ ] 5.1 Move `get_meeting_speakers`, `get_cluster_channel`, `set_user_binding`, `set_auto_binding_if_unbound`, `confirm_speaker_binding`, `get_cluster_centroids`, `rebind_cluster` into `speaker/binding.rs`, plus the `ClusterCentroid` struct; declare `mod binding;` and `pub use binding::*;` in `mod.rs`. Move the 6 tests listed in design.md's `binding.rs` row; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `68`.

## 6. Extract `voiceprints.rs`

- [ ] 6.1 Move `list_voiceprints`, `reject_voiceprint`, `reconfirm_voiceprint`, `verify_voiceprint`, `verify_speaker`, `verify_meeting_caches`, `get_voiceprint_audio`, `clear_all_voiceprints`, `purge_unconfirmed_caches` into `speaker/voiceprints.rs`, plus `VoiceprintRow`, `SpeakerVoiceprints`, `MeetingVoiceprints`, `VoiceprintBrowser`, `RejectResult`, `ClearAllResult`, `PurgeUnconfirmedCachesResult`; declare `mod voiceprints;` and `pub use voiceprints::*;` in `mod.rs`. Move the 10 tests listed in design.md's `voiceprints.rs` row; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `68`, and `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep "speaker/voiceprints.rs" | grep -c "unnecessary use of \`clone\`"` is `0`.

## 7. Extract `merge.rs`, fix the N+1

- [ ] 7.1 Move `replace_speaker` and `preview_replace_speaker` into `speaker/merge.rs`, plus the `ReplaceResult` struct; declare `mod merge;` and `pub use merge::*;` in `mod.rs`; move `replace_preserves_user_binding_and_override_and_is_atomic`; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `68`.
- [ ] 7.2 In `merge.rs`, replace `replace_speaker`'s per-cluster `COUNT(*)` loop (transcript-count section) with the single `(meeting_id, speaker) IN (VALUES ...)` / `GROUP BY` query from design.md D4, run inside the existing transaction (`&mut *tx`); apply the identical replacement to `preview_replace_speaker` (run against `pool` since it has no transaction); add the D5 documenting comment immediately after `tx.commit().await?;` in `replace_speaker` explaining why the re-match loop stays outside the transaction; verify: `cargo check -p meetily`.
- [ ] 7.3 Add `replace_speaker_transcript_count_matches_per_cluster_sum` to `merge.rs`'s tests: seed 2+ meetings with several auto-bound clusters each (including one cluster label that repeats across two different meetings with different transcript counts, to exercise exact-pair matching), call `replace_speaker`, and assert `affected_transcripts` equals the sum computed by a straightforward per-pair loop over the same fixture data computed independently in the test; also call `preview_replace_speaker` on an equivalent unmodified fixture and assert the same total; verify: `cargo test -p meetily --lib speaker::merge` passes and `cargo test -p meetily --lib speaker -- --list | grep -c ": test"` prints `69` (68 + this new test).

## 8. Extract `overrides.rs`

- [ ] 8.1 Move `set_transcript_override`, `clear_transcript_override`, `get_transcript_cluster`, `get_transcript_time_info`, `resolve_cluster_by_time_overlap`, `get_transcript_display_name`, `apply_turn_overrides`, `apply_cluster_binding_overrides`, `get_display_names` into `speaker/overrides.rs`; declare `mod overrides;` and `pub use overrides::*;` in `mod.rs` (no new structs — all return primitives/`HashMap`/`Option`). Move the 11 tests listed in design.md's `overrides.rs` row; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `69`, and `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep "speaker/overrides.rs" | grep -c "unnecessary use of \`clone\`"` is `0`.

## 9. Extract `stats.rs`

- [ ] 9.1 Move `storage_stats` into `speaker/stats.rs`, plus the `SpeakerStorageStats` struct; declare `mod stats;` and `pub use stats::*;` in `mod.rs`. Move `storage_stats_match_raw_sum`, `storage_stats_disambiguates_provenanced_prototype_and_cache`, `voiceprint_storage_stats_separates_audio_bytes`; verify: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` passes, test count still `69`.

## 10. Final verification

- [ ] 10.1 Confirm `mod.rs` now contains only: shared imports, the 4 constants, `pub struct SpeakerRepository;`, the 7 `mod` declarations (+ `test_support`), and the 7 `pub use` re-exports — no leftover `impl SpeakerRepository` block or test module; verify: `grep -c "impl SpeakerRepository" frontend/src-tauri/src/database/repositories/speaker/mod.rs` is `0` and `wc -l frontend/src-tauri/src/database/repositories/speaker/mod.rs` shows well under 100 lines.
- [ ] 10.2 Confirm no caller outside `speaker/` changed: `git diff --stat -- frontend/src-tauri/src/audio/diarization.rs frontend/src-tauri/src/audio/online_diarization.rs frontend/src-tauri/src/audio/recording_commands.rs frontend/src-tauri/src/database/speaker_commands.rs` prints no output (zero diff in those 4 files); verify: command output is empty.
- [ ] 10.3 Run the full verification pass: `cargo check -p meetily`, `cargo test -p meetily --lib speaker` (69 tests, all passing), `cargo test -p meetily --lib speaker -- --list | grep -c ": test"` prints `69`, and `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep "repositories/speaker" | grep -c "unnecessary use of \`clone\`"` prints `0`; verify: all four commands succeed with the stated output.
- [ ] 10.4 Run `openspec validate 06-split-speaker-repository --strict` and `openspec status --change 06-split-speaker-repository`; verify: validate passes with no errors and status shows every task complete.
