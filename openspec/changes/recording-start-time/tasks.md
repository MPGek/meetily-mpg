## 1. Database migration and model

- [x] 1.1 Add SQLx migration adding nullable `meetings.started_at` with backfill `started_at = created_at`, verifying it applies cleanly on a fresh and existing DB
- [x] 1.2 Add `started_at` to `MeetingModel` (`#[sqlx(default)]`) plus clarifying comments on `started_at` vs `created_at`, verifying `cargo check -p meetily` passes

## 2. Write paths

- [x] 2.1 In `TranscriptsRepository::save_transcript`, resolve start from the folder's `metadata.json created_at` (fallback `now()`), verifying a stop at 15:30 for a 14:00 recording stores `started_at` = 14:00 and corrupt/missing metadata still saves with fallback
- [x] 2.2 In the audio-import insert, use the audio file's mtime as `started_at` (fallback import moment), verifying normal and unreadable-mtime imports both store non-empty `started_at`
- [x] 2.3 Verify the crash-recovery save path reuses one of the above (or apply the same metadata.json read there), verifying a recovered recording keeps its original start, not the recovery moment

## 3. Read path and display

- [x] 3.1 Expose `started_at` in the meetings list DTO (`Meeting`, batch-loaded like `tags`), keeping `created_at DESC` ordering, verifying payload shape via invoke smoke test
- [x] 3.2 Prefer `started_at ?? created_at` at date display sites (sidebar rows first), verifying new meetings show start time and legacy rows fall back to `created_at`

## 4. Verification

- [x] 4.1 Run `cargo test --lib`, `cargo check -p meetily`, and frontend `tsc --noEmit`, verifying zero new errors
- [ ] 4.2 Manual pass: record a meeting (confirm list shows start, not stop), import audio (confirm file-mtime date), reload app, verifying persistence end to end
