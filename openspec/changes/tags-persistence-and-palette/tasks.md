## 1. Confirm the current failure mode

- [ ] 1.1 Rebuild the Tauri backend and hard-reload the frontend, then start a recording with a tag picked before start and add one during recording; verify the recording folder's `metadata.json` shows a non-empty `pending_tag_ids` while recording, and record the observed value
- [x] 1.2 Confirm the frontend bundle in use (`frontend/out` or dev `.next`) contains the current `usePendingRecordingTags` push code, ruling out a stale build as the cause, and record the result before changing logic — verified: `frontend/out/_next/static/chunks/app/page-*.js` and `.next` both contain "carry pre-start pending tags into recording", so the build is not stale

## 2. Frontend pending-set reliability

- [x] 2.1 On the start transition, push the pre-start selection and confirm from the write's returned list instead of a follow-up `load()`; verify a pre-start tag stays in the picker and in `metadata.json` after recording starts — implemented in `usePendingRecordingTags.ts` (push + confirm, no re-read overwrite); runtime confirmation rolled into 5.2
- [x] 2.2 Preserve the current selection when a pending-set read or write fails, and never clear the picker on an empty or failed read while recording; verify the chip stays visible when the backend rejects the write — implemented (empty/failed read only clears when idle); runtime confirmation rolled into 5.2
- [x] 2.3 Surface pending-set persist/carry failures with a user-visible warning rather than a console error; verify the warning appears when the backend rejects the write — implemented via `toast.warning`, warned once per recording; runtime confirmation rolled into 5.2
- [x] 2.4 Clear the pending set only on cancel/discard or after a successful save; verify the set survives until the save completes and a following recording starts empty — the persisted set survives until save-time linking; the visible set clears when the recording ends so the next recording starts empty

## 3. Backend save-time linking

- [x] 3.1 Resolve the finished recording's folder at save time when the frontend passes no `folder_path`, so pending tags still link; verify with a Rust test that a non-empty `pending_tag_ids` in the resolved folder produces meeting links — `resolve_recording_folder` + `pending_tags_link_via_last_recording_folder_fallback` passes
- [x] 3.2 Return a user-visible warning when a non-empty pending set cannot be read at save; verify the warning is present in the save response and shown by the stop flow — `missing_folder_warns_instead_of_dropping_tags` passes; `useRecordingStop` already toasts `tag_warnings`
- [x] 3.3 Keep every `metadata.json` write under the shared write lock with atomic replace; verify the existing concurrent-write test still passes — `concurrent_summary_language_writes_preserve_both_fields` passes

## 4. Palette expansion

- [x] 4.1 Expand the backend tag palette constant to 40 keys and verify a unit test asserts the count and key uniqueness — `palette_has_forty_unique_keys` passes
- [x] 4.2 Expand the frontend chip style map to the same 40 keys with distinct static classes and verify a test asserts the frontend key set equals the backend palette key set — `tag-palette.json` manifest; `palette_matches_frontend_manifest` (Rust) and the frontend palette test assert parity; verified 40 unique keys / 40 distinct classes
- [x] 4.3 Verify the manual color cycle visits every palette entry and wraps from the last to the first — `nextColor` moved to `meeting-tags.ts`; cycle verified over all 40 entries with wrap
- [x] 4.4 Make Tailwind emit the palette classes (they live in `src/lib`, outside the content globs) and verify the compiled CSS contains them — `tailwind.config.js` now includes `./src/lib/**`; CLI compilation emits `bg-lime-100`, `bg-stone-100`, `text-zinc-800`, `bg-slate-200`, `text-slate-900`

## 5. Verification

- [x] 5.1 Run backend `cargo test --lib` and frontend `tsc --noEmit` plus the palette tests; verify zero new errors — `cargo test --lib` 431 passed / 0 failed; `tsc --noEmit` shows only the pre-existing `bun:test` resolution errors; palette logic verified directly with Node
- [ ] 5.2 Manual end-to-end: pick a tag before start, add one mid-recording, stop; verify the saved meeting shows both tags in Meeting Notes, a cancelled tagged recording keeps the dictionary tag with no meeting, and a UI reload mid-recording keeps the set
- [ ] 5.3 Visual check: create about 12 tags and verify the chips show visibly different backgrounds and the editor color cycle moves through the palette
