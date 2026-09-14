## 1. Backend pending-set plumbing

- [x] 1.1 Add pending-tag commands (read/update the `metadata.json` pending key, init `[]` at recording setup), verifying round-trip `set → get` and init-on-start via unit test
- [x] 1.2 Link pending tag ids at meeting save (best-effort per tag, stale ids skipped with warning, meeting save never fails on tags), verifying a 2-tag pending set lands on the new meeting and a deleted-mid-recording id warns without failing

## 2. Home UI before and during recording

- [x] 2.1 Add pre-start tag picker on Home (existing tags + create-new, mirrors to backend on every change), verifying picks survive to recording start and a fresh start begins empty
- [x] 2.2 Reuse the same pending-set editor in the in-recording panel (add/toggle/create/remove), verifying mid-recording edits change what gets linked at stop and never touch stored meetings
- [x] 2.3 Clear the pending set on cancel/discard (pre-created dictionary tags remain), verifying the next recording starts empty with no orphan links

## 3. Verification

- [x] 3.1 Run `cargo test --lib`, `cargo check -p meetily`, and frontend `tsc --noEmit`, verifying zero new errors
- [ ] 3.2 Manual pass: pick tags → record → stop (meeting pre-tagged in list), tag mid-recording, cancel a tagged recording (dictionary keeps tag, no meeting), reload UI mid-recording (pending set intact), verifying end to end
