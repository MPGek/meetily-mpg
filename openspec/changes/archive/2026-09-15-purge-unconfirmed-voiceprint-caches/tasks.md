## 1. Repository operation

- [x] 1.1 Add a `PurgeUnconfirmedCachesResult` struct (deleted cache count, deleted embedding bytes, deleted clip count, deleted clip bytes) next to `ClearAllResult` in `frontend/src-tauri/src/database/repositories/speaker.rs`; verify it derives `Serialize`/`Deserialize` like `ClearAllResult` and `cargo check` passes.
- [x] 1.2 Implement `SpeakerRepository::purge_unconfirmed_caches` as a single transaction that first aggregates `COUNT(*)`, `SUM(LENGTH(embedding))`, `COUNT(audio_blob IS NOT NULL)`, and `SUM(LENGTH(audio_blob))` over `speaker_id IS NULL`, then deletes exactly that predicate; verify with a unit test that a fixture containing both prototypes and caches leaves `SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id IS NOT NULL` unchanged and `WHERE speaker_id IS NULL` at zero.
- [x] 1.3 Verify the ownership boundary is not crossed: unit test asserting that a purge leaves every registry speaker, every `meeting_speakers` row (including `centroid`, `matched_by`, `match_score`), every `meeting_expected_speakers` row, and every `transcripts.speaker_override_id` value byte-for-byte unchanged (`cargo test --manifest-path frontend/src-tauri/Cargo.toml purge`).
- [x] 1.4 Verify reported totals match the deleted rows: unit test with known embedding sizes and two cache rows carrying `audio_blob` asserting `deleted_embedding_bytes`, `deleted_clip_count`, and `deleted_clip_bytes` equal the pre-purge aggregates, and that an empty cache layer returns all zeros without error.

## 2. Command and registration

- [x] 2.1 Add the `purge_unconfirmed_caches` Tauri command in `frontend/src-tauri/src/database/speaker_commands.rs` wrapping the repository call and mapping errors to a `Failed to purge unconfirmed caches: ...` string; verify `cargo check` passes.
- [x] 2.2 Do NOT clear the in-memory `PrototypeStore` in the command, and add a short comment recording why (design D4: caches are never loaded into the store, and clearing would discard in-session user bindings); verify the comment is present and the command body contains no `ONLINE_DIARIZATION_STORE` access.
- [x] 2.3 Register the command in the `invoke_handler` list in `frontend/src-tauri/src/lib.rs` next to `clear_all_voiceprints`; verify `cargo check` passes and a frontend `invoke('purge_unconfirmed_caches')` resolves instead of failing with an unknown-command error.

## 3. Frontend

- [x] 3.1 Add a typed `purgeUnconfirmedCaches()` wrapper to `frontend/src/services/recordingService.ts` next to `clearAllVoiceprints`, returning the four-field result shape; verify `npx tsc --noEmit` in `frontend` passes.
- [ ] 3.2 Add the "Remove unconfirmed caches" control to the Storage card in `frontend/src/components/VoiceprintBrowser.tsx`, adjacent to the existing "Remove all" button, disabled when `stats.cache_count === 0`; verify by rendering the browser with zero and with nonzero cache counts and confirming the disabled state.
- [ ] 3.3 Add a confirmation dialog modelled on `ConfirmClearAllDialog` that states the cache count, the affected voice-clip count, and that the removal cannot be undone; verify the dialog renders these three facts and that cancelling performs no invoke call.
- [ ] 3.4 On confirm, invoke the command, stop and reset clip playback state (`player.pause()`, `playingRowId`, `failedRowId`, `pendingRange`) as `handleClearAllConfirm` does, then reload both `storage_stats` and `list_voiceprints`; verify the unconfirmed branch is empty, the summary shows zero caches with the reduced size, and the confirmed-speaker groups are unchanged.

## 4. Regression tests

- [x] 4.1 Add a repository test proving a purge does not break enrollment semantics for the remaining data: after purging, `enroll_cluster` for a formerly cached cluster returns 0 without error (mirroring the existing no-cache case at `speaker.rs:1984`), while `load_prototypes` still returns the pre-existing prototypes; verify `cargo test --manifest-path frontend/src-tauri/Cargo.toml` passes.
- [x] 4.2 Add a repository test proving prototypes whose provenance points at a meeting that still has caches survive a purge and still load for recognition; verify the test fails if the delete predicate is broadened to drop `speaker_id IS NOT NULL` rows.

## 5. Verification

- [x] 5.1 Run `cargo test` and `cargo clippy` in `frontend/src-tauri`, plus `npx tsc --noEmit` and `pnpm -C frontend lint`, verifying zero new errors or warnings in the touched modules.
- [x] 5.2 Run `openspec validate purge-unconfirmed-voiceprint-caches --strict` and `openspec validate purge-unconfirmed-voiceprint-caches` with no errors.
- [ ] 5.3 Manual pass: open Settings with a database containing legacy caches, confirm the control shows the affected counts, cancel and confirm the browser is unchanged, then confirm and verify the unconfirmed branch is empty, the storage summary dropped, the confirmed speakers and their prototypes still display and still auto-match on a meeting re-match, and clip playback of a removed row has stopped.
