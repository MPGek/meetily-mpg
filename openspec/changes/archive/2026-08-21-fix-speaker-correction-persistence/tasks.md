## 1. Backend: enroll voiceprints on inline corrections

- [x] 1.1 In `speaker_commands.rs::assign_block_speaker`, after `set_transcript_override`, resolve the block's `(meeting_id, cluster_label)` via `SpeakerRepository::get_transcript_cluster` and call `enroll_cluster` for the assigned speaker so a single offline block correction enrolls the person (no-op on legacy/no-cache meetings).
- [x] 1.2 Confirm `assign_speaker` / `apply_block_speaker_to_cluster` still call `enroll_cluster` (unchanged) and that `find_or_create_by_name` results flow into enrollment.
- [x] 1.3 Add/extend Rust tests in `speaker.rs` covering: offline single-block correction enrolls cached exemplars; legacy meeting without caches labels without error and enrolls 0.

## 2. Backend: new "confirm correct" command

- [x] 2.1 Add `SpeakerRepository::confirm_speaker_binding` (or similar) that marks an auto binding user-confirmed without changing the name: per-block sets `transcripts.speaker_override_id`; per-cluster flips `meeting_speakers.matched_by='user'` and clears `match_score`. Must NOT insert new `speaker_embeddings`.
- [x] 2.2 Expose it as a `#[tauri::command]` (e.g. `confirm_block_speaker`) in `speaker_commands.rs` and register it in `lib.rs`.
- [x] 2.3 Add Rust tests: confirming a recognized cluster clears score and sets matched_by='user'; confirming a single block sets the override; confirming does not duplicate prototypes.

## 3. Backend: harden live label persistence

- [x] 3.1 In `recording_commands.rs`, make `assign_live_speaker` return an actionable error (not a `warn!` + fake success) when no live prototype store / online session is active.
- [x] 3.2 Ensure `finalize_online_session` runs for every live-diarized stop; remove reliance on a frontend flag that can be false after live bindings were made.
- [x] 3.3 In `finalize_online_session`, after writing `meeting_speakers` user bindings and `speaker_override_id`, also write the stored `transcripts.speaker` for user-bound blocks so the DB row carries the user's identity (no longer depends solely on the render-time join).
- [x] 3.4 Add Rust tests: finalize persists a user cluster binding such that reopening yields the user's name with user provenance; single-turn override survives; a live assignment without an active store fails loudly.

## 4. Frontend: offline suffix clears in place

- [x] 4.1 In `usePaginatedTranscripts.updateSpeakerLabel`, set `speaker_matched_by:'user'` and clear `speaker_match_score` for edited blocks (mirror `applyLiveSpeakerLabel`).
- [x] 4.2 Remove the `speaker_label === label` short-circuit in `updateSpeakerLabel` so re-selecting the same name is not a silent no-op.

## 5. Frontend: confirmation affordance and suffix behavior

- [x] 5.1 In `VirtualizedTranscriptView`'s `SpeakerLabel`, treat re-selecting the already-displayed name as a confirmation: invoke the confirm command (backend 2.x) instead of no-op, and clear the `(auto)` decorators locally.
- [x] 5.2 Add an explicit confirm affordance on auto-decorated labels (or reuse re-selection) and ensure both offline and live views call the confirm path with visible "saved" feedback (suffix removed).
- [x] 5.3 Add frontend wiring in `recordingService.ts` for the new confirm command.

## 6. Verification

- [x] 6.1 Run backend Rust test suite (`cargo test` in `frontend/src-tauri`) — speaker/diarization tests pass.
- [x] 6.2 Run frontend lint/typecheck and confirm no regressions in `TranscriptContext` / `usePaginatedTranscripts`.
- [x] 6.3 Manual smoke: offline single-block edit enrolls voiceprint + drops suffix; live correction survives stop/reopen; confirming an auto label visibly clears the suffix.

## 7. Voiceprint browser UX refinements

- [x] 7.1 In `VoiceprintBrowser.tsx`, replace the raw `Bytes: {total_bytes}` storage display (line ~431) with a human-readable size (reuse the `formatBytes` helper matching `DiarizationSettings.tsx`), so the summary shows MB and agrees with the general-tab storage section.
- [x] 7.2 Change the voiceprint browser default load state to all-collapsed (empty `expanded` set) instead of the current default-expanded (line ~252-256), keeping the per-group toggle and expand-all/collapse-all controls working.
- [x] 7.3 Verify: run `tsc --noEmit` / lint in `frontend`; confirm the storage summary shows MB and groups load collapsed, and that expand-all still reveals all groups.
