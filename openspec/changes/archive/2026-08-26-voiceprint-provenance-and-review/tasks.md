## 1. Schema migration

- [x] 1.1 Add migration `frontend/src-tauri/migrations/20260819xxxx_add_voiceprint_provenance.sql` that rebuilds `speaker_embeddings`: adds `audio_start_time REAL` and `audio_end_time REAL`, relaxes the ownership CHECK to allow a prototype (`speaker_id IS NOT NULL`) to carry any provenance, preserves legacy NULL-provenance rows, recreates the two existing indexes, and adds `idx_speaker_embeddings_cache_speaker (meeting_id, cluster_label, speaker_id)`
- [x] 1.2 Verify the migration is data-preserving: existing prototype rows and cache counts are identical after `sqlx::migrate!` in a scratch DB seeded from the current schema

## 2. Model + provenance threading

- [x] 2.1 Add `start_secs`/`end_secs` to `Exemplar` (speaker.rs:52) and `start_secs`/`end_secs` to `ClusteredEmbedding` (diarization.rs:542); add `audio_start_time`/`audio_end_time` fields to the `SpeakerEmbedding` model (models.rs:165)
- [x] 2.2 Offline path: set start/end from `DiarizationSegment` at both `ClusteredEmbedding` build sites (diarization.rs:796-799 and 1143-1146) instead of only computing `duration_secs`
- [x] 2.3 Online path: set start/end from the `(start, end, embedding)` triples at both build sites (online_diarization.rs:854-857 and 872-875) and ensure the cache write receives them
- [x] 2.4 Update `write_cluster_cache` (speaker.rs:195) to INSERT `audio_start_time`/`audio_end_time` from each exemplar; verify a `start_secs` present implies `end_secs` at write time
- [x] 2.5 Verification test: run offline and online diarization on a short stereo fixture and assert cached `speaker_embeddings` rows contain expected `audio_start_time`/`audio_end_time`, and that those times align with transcript block `audio_start_time` within tolerance

## 3. Enrollment preserves provenance

- [x] 3.1 `enroll_cluster` (speaker.rs:359): change the reparent UPDATE to set only `speaker_id` — remove `meeting_id = NULL, cluster_label = NULL`
- [x] 3.2 `enroll_embeddings_from_buffer` (speaker.rs:397): insert `audio_start_time`/`audio_end_time` from the buffer chunks' `(start, end)` it already filters on
- [x] 3.3 Update the `write_cache_and_enroll_reparents_best_k` test plus new assertions: enrolled prototypes retain `meeting_id`/`cluster_label`/times; `load_prototypes` (speaker.rs:451) still returns them and is unchanged in filter behavior

## 4. Meeting deletion scope + storage stats

- [x] 4.1 `meeting.rs:376`: change meeting deletion to `DELETE FROM speaker_embeddings WHERE meeting_id = ? AND speaker_id IS NULL` (remove only unassigned caches)
- [x] 4.2 `storage_stats` (speaker.rs:545): define `cache_count` as `speaker_id IS NULL AND meeting_id IS NOT NULL` so provenanced prototypes don't inflate it; keep `prototype_count = speaker_id IS NOT NULL`
- [x] 4.3 Update storage-stats tests and add a test where a provenanced prototype plus a cache coexist and counts disambiguate correctly

## 5. Review browser backend

- [x] 5.1 Add `VoiceprintRow` (id, model, channel, duration, owner kind, meeting_id, cluster_label, audio times, speaker_id, score-origin if available) and a grouped `VoiceprintBrowser` shape (speakers + counts + per-meeting unconfirmed) in `database/repositories/speaker.rs`
- [x] 5.2 Add `list_voiceprints` repo query supporting per-speaker prototypes and per-meeting unconfirmed caches, lazy/paginated by speaker, resolving meeting titles via LEFT JOIN with "deleted meeting" fallback
- [x] 5.3 Register Tauri commands in `speaker_commands.rs` + `lib.rs`: `list_voiceprints(pool)`, and a filtered variant `list_voiceprints(speaker_id | unconfirmed)`
- [x] 5.4 Verification: unit tests for the grouped query shapes (speaker prototype rows, per-meeting cache grouping, legacy row with NULL provenance)

## 6. Voiceprint playback

- [x] 6.1 Extend the frontend `useAudioPlayer` hook with `playRange(start, end)`: seek to `start`, play, and a `timeupdate` listener that pauses at `end` (reuse `get_meeting_audio_path` + asset-protocol streaming; no new engine)
- [x] 6.2 Add frontend voiceprint browser component consuming the grouped payload, rendering speaker subtrees + unconfirmed meetings as collapsible groups, provenance columns, and "source unavailable" for rows without provenance
- [x] 6.3 Wire play actions: disabled when no audio file or no timecodes; render storage totals from `speaker_storage_stats` and refresh after review actions
- [x] 6.4 Verification: manual check that play-from-range streams, seeks to start, pauses at end; disabled states render for no-audio/no-timecode rows
- [x] 6.5 Add per-group collapse/expand toggle for each speaker and each meeting group in the voiceprint browser (accessible button, chevron, `aria-expanded`, keyboard operable), default expanded, no data reload on toggle
- [x] 6.6 Add expand-all / collapse-all controls that affect all speaker and meeting groups at once; header/summary remain visible and bulk actions are no-ops when already in target state or when groups are empty
- [x] 6.7 Verification: verify groups collapse/expand individually and bulk controls work, headers/counts remain visible, no reload, keyboard accessible

## 7. Rejection and re-confirmation backend

- [x] 7.1 Repo: `reject_voiceprint(id, permanent: bool)` — demote (`UPDATE ... SET speaker_id = NULL`) or `DELETE`, returning the affected owner so the UI can warn when a speaker's set empties
- [x] 7.2 Repo: `reconfirm_voiceprint(id, speaker_id)` — set owner and enforce per-person cap (reuse `enforce_prototype_cap`), no provenance change
- [x] 7.3 Repo: `replace_speaker(source, target: Option<String>)` in one transaction — collect auto-bound `meeting_speakers`, re-bind to target or unset, delete source prototypes, re-match affected meetings from centroids, return affected meeting/cluster/transcript counts
- [x] 7.4 Commands + registration: `reject_voiceprint`, `reconfirm_voiceprint`, `replace_speaker` in `speaker_commands.rs`/`lib.rs`
- [x] 7.5 Unit tests: reject demotes and is excluded from recognition; reconfirm enforces cap; replace preserves `matched_by='user'` and per-transcript overrides; replace failure leaves everything unchanged (rollback)

## 8. Rejection and re-confirmation UI

- [x] 8.1 Row actions in the voiceprint browser: reject (with a "permanently delete?" choice), reconfirm-to-speaker picker via the shared person-picker dialog (searchable list of other persons, quick filter, create-new), and "replace speaker across meetings" via the same dialog (with anonymous option) for confirmed sets
- [x] 8.2 Replacement flow: after the shared picker resolves a target (or anonymous), show confirmation dialog with affected meeting/cluster/transcript counts before executing `replace_speaker`
- [x] 8.3 Refresh browser + storage totals after any review action; update per-speaker counts
- [x] 8.4 Verification: manual end-to-end — reject a prototype, confirm it reappears in the unconfirmed branch under its meeting; replace Alice→Bob and confirm auto-view labels change in existing meetings while user-bound ones stay
- [x] 8.5 Verification: reconfirm and replace both open the same person-picker (search, filter, create-new); replace picker excludes the source speaker and offers anonymous, and reuses existing `list_speakers`/`find_or_create_by_name` paths without a separate prompt

## 9. Spec-close verification

- [x] 9.1 Run `cargo test` and `cargo clippy` in `frontend/src-tauri`; run frontend lint/build; fix regressions
- [x] 9.2 Confirm recognition tests (`best_match`, rematch) are unchanged and pass, proving provenance doesn't alter matching
- [x] 9.3 Run `openspec validate --change voiceprint-provenance-and-review` until clean


