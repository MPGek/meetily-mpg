# Tasks

## 1. Sub-row resolution (frontend, pure helpers)

- [x] 1.1 In `frontend/tests/lib/live-speaker-labels.test.ts`, add failing `resolveLiveBlocks` cases for the live-speaker-labels delta scenarios: an edit on one sub-row leaves same-cluster siblings alone (A, B, A); a later sub-row edit does not revert an earlier one; confirm changes only the confirmed sub-row's `matched_by`; re-attribution to another cluster keeps the override; the latest override wins; a pin from before the split still applies after a sub-row edit. Verify with `bun test tests/lib/live-speaker-labels.test.ts` that the new cases fail and the existing ones pass.
- [x] 1.2 Change `resolveLiveBlocks` in `frontend/src/lib/live-speaker-labels.ts` so overrides match by time overlap and `sourceDevice` rather than cluster, searched newest-first (design D2). Add `sourceDevice` to the override shape. Verify that the 1.1 cases covering resolution pass and all prior cases still pass.

## 2. Sub-row edit scope (frontend, context and view)

- [x] 2.1 In `frontend/src/contexts/TranscriptContext.tsx`, separate the freeze set used by `rematchTranscripts` from the pin map (design D1). A `subRow` edit in `applyLiveSpeakerLabel` records a window override carrying the parent's `source_device` and freezes the parent, without writing the pin. An unsplit-block edit keeps pin + freeze, and apply-to-all is unchanged. Also clear the new set on `recording-started`. Verify with a `rematchTranscripts` test that a frozen-but-unpinned id is not re-matched, and with `bunx tsc --noEmit`.
- [x] 2.2 Pass the `subRow` flag from the split-block render path of `SpeakerLabel` in `frontend/src/components/VirtualizedTranscriptView.tsx`, through `onUpdateSpeakerLabel` and `frontend/src/app/_components/TranscriptPanel.tsx`, for both assign and confirm. The offline `meetingId` path stays unchanged. Verify with `bunx tsc --noEmit` and `bun test tests/`.

## 3. Voiceprint deduplication (backend)

- [x] 3.1 In `frontend/src-tauri/src/database/repositories/speaker.rs`, add a test that enrolls the same buffer chunk for one speaker twice through `enroll_embeddings_from_buffer` and asserts a single row. Add another asserting that the same chunk enrolled for a second speaker is kept. Verify that the first test fails before the fix.
- [x] 3.2 Make `enforce_prototype_cap` collapse the speaker's duplicates on `(meeting_id, channel, audio_start_time, audio_end_time, embedding)` before counting. Keep the first row by `is_verified DESC, audio_blob IS NOT NULL DESC, created_at ASC`, carrying a clip over from a deleted copy (design D3). Add a test that duplicates never evict a distinct shorter prototype at the cap. Verify with `cargo test -p meetily --lib speaker` (3.1 passes, `enrollment_enforces_per_person_cap` still passes).
- [x] 3.3 In `enroll_embeddings_from_buffer`, drop candidates that already exist for the speaker before cutting clips, so the returned count excludes them. Verify with a test asserting the returned count is 0 on re-enrollment.
- [x] 3.4 In `persist_session` (`frontend/src-tauri/src/audio/diarization/engine.rs`), enroll per `(speaker_id, channel)` from the union of that person's override windows, deduplicated by chunk window (design D4). `apply_turn_overrides` keeps the full ordered list. Verify with a test where three overrides overlap one buffered chunk and exactly one prototype results.

## 4. Existing duplicates (migration)

- [x] 4.1 Add `frontend/src-tauri/migrations/<timestamp>_dedupe_speaker_voiceprints.sql`. For rows with `speaker_id IS NOT NULL`, it copies the clip onto the kept row where needed, sets `is_verified` on the kept row if any copy was verified, and deletes the other copies (design D5). Verify with a repository test that seeds 13 identical rows plus one distinct row, runs the migrations, and asserts 2 rows remain, verified and with a clip.
- [x] 4.2 Run the migration against a copy of `%APPDATA%\com.meetily.ai\meeting_minutes.sqlite` (with `-wal`/`-shm`, never the live file). Verify that Vasil Boika goes from 64 to 15 rows, that no speaker keeps a duplicate group, and that non-duplicate counts are unchanged.

## 5. End-to-end check

- [x] 5.1 In a live Fast-mode recording, produce a split block with repeated clusters. Edit one sub-row, then confirm another. Verify that only the touched sub-rows change and no checkmark appears or disappears elsewhere. After stop, verify in the voiceprint browser that the named person gained no duplicate voiceprints.
- [x] 5.2 Try to reproduce an `(auto) NN%` sub-row label rendered without its confirm checkmark (seen on one row of the original screenshot). If it reproduces, record the cause in design.md and fix it or split it out. If it does not, note that here. Result (2026-10-01): the user reported the live check passed; the missing checkmark did not reproduce.
- [x] 5.3 Run `graphify update .` after the code changes and verify that it completes.
