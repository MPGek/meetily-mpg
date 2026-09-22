# Tasks

Standing verification for every task: `cargo test -p meetily --lib -- --skip system_audio_commands` (the skip works around the pre-existing crash documented in archived change 05, task 1.1) stays at **450 passed / 9 ignored plus whatever this change adds**, and `cargo clippy -p meetily --all-targets --message-format=short` introduces no new warning above its **221-line** baseline. The one environment failure, `audio::playback_monitor::tests::test_get_output_device`, is unrelated and pre-existing.

## 1. Storage for the row-level match

- [ ] 1.1 Add the additive migration `frontend/src-tauri/migrations/<date>_add_transcript_row_recognition.sql` creating `transcripts.speaker_auto_id TEXT NULL` and `transcripts.speaker_auto_score REAL NULL`; verify `cargo test -p meetily --lib database` passes (migrations run in the in-memory pool the repository tests use) and that a `pragma table_info(transcripts)` on a freshly migrated database lists both columns as nullable
- [ ] 1.2 Add repository writes for the row-level match in `database/repositories/speaker.rs` — set the pair for one transcript id, and clear it for a whole meeting — leaving `speaker_override_id`, `speaker_label` and `meeting_speakers` untouched; verify with a repository test over the in-memory pool that writing then clearing leaves the row's other speaker columns byte-identical

## 2. Stop-time per-row matching

- [ ] 2.1 Add the per-row matching pass in `audio/diarization/persist/clusters.rs`: for each transcript row of the meeting, select the session embeddings whose `(start, end)` overlaps the row and whose channel matches the row's `source_device`, score them with the existing `best_match_with_threshold` over the same candidate prototypes cluster recognition uses, and record the best above-threshold match on the row; verify with a unit test over fixture embeddings that a row surrounded by another speaker's evidence records that speaker while the cluster binding is left alone
- [ ] 2.2 Call the pass from `DiarizationEngine::persist_session` after the existing cluster persistence, feeding it the session's retained `mic_embeddings`/`sys_embeddings`; verify the existing `persist_session` path still reports its live-binding and enrolled counts unchanged for a session with no prototypes, and that a session whose rows have no overlapping embedding records no row-level match
- [ ] 2.3 Confirm the pass never runs on the offline path: verify by grep that `persist_and_recognize_session`'s batch caller does not invoke it and by a test that an offline-diarized meeting has `speaker_auto_id IS NULL` on every row

## 3. Display precedence

- [ ] 3.1 Extend `TRANSCRIPT_DISPLAY_SELECT` in `database/repositories/meeting.rs` so the resolved name is `COALESCE(so.name, <row-auto name suppressed when the cluster is user-bound>, s.name, t.speaker_label)`, and the provenance/score columns report the level that won; verify with repository tests covering each precedence scenario in the spec delta: override wins, row-level beats an auto cluster, a user-bound cluster beats row-level, no row match falls through to the cluster, legacy label still last
- [ ] 3.2 Verify no other surface resolves names on its own: grep for `speaker_override_id` and `meeting_speakers` joins outside this projection and confirm each remaining site either reads the projection or is an editing path, not a display path

## 4. Re-match consistency

- [ ] 4.1 Extend `rematch_meeting_speakers` (`audio/diarization/commands.rs`) to recompute the meeting's row-level matches from the persisted `speaker_embeddings` rows (using their `audio_start_time`/`audio_end_time`) under the current candidate set, and to clear the matches that set no longer supports; verify with a test that removing a speaker from the allowlist and re-matching clears the row-level names that depended on them while a user override and a user-bound cluster survive untouched
- [ ] 4.2 Verify re-match reads no audio: confirm by test or inspection that the new path touches only `speaker_embeddings`, `meeting_speakers` and `transcripts`, with no decode or model call

## 5. Verification on the measured meeting

- [ ] 5.1 Reproduce the proposal's measurement against the shipped implementation on `meeting-c39544ca-9eab-4834-a790-98fe389966c9`: verify that of the 18 rows currently displaying Greg, **10** resolve to Alex Shingel (6), Mikhail Shashalevich (3) and Siarhei Hryb (1); that **9** rows agree with their cluster binding; that the **13** below-threshold rows keep today's name; and that **34 of 35** rows had an overlapping embedding — recording any deviation in this line
- [ ] 5.2 Verify a user's decisions are still absolute on that meeting: set a per-block override and confirm one cluster binding as correct, then verify the override row and every row of the confirmed cluster display the user's names regardless of their row-level matches
- [ ] 5.3 Verify the offline path is unmoved: re-run `uv run --project eval subset` and confirm the offline gate still reports voxconverse DER 8.40 / Conf 3.89 and passes, with the online bounds unchanged
- [ ] 5.4 Run `openspec validate per-row-speaker-recognition --strict` and `openspec status --change per-row-speaker-recognition`; verify validation passes and every artifact is reported done
