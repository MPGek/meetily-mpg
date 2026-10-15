## 1. Database migration and models

- [x] 1.1 Add SQLx migration for `meeting_tags` + `meeting_tag_links` (NOCASE unique index, composite PK, indexes) and verify `sqlx migrate run` applies cleanly on a fresh and existing DB
- [x] 1.2 Add `Tag`/`TagLink` models and extend meetings list read to batch-load tags per meeting, verifying legacy meetings return empty `tags` with valid `created_at`
- [x] 1.3 Extend `delete_meeting_with_transaction` to cascade `meeting_tag_links` and verify no orphan links remain after deleting a tagged meeting

## 2. Backend tag commands and meetings payload

- [x] 2.1 Implement `list_tags` (with usage counts), `create_tag`, `rename_tag`, `set_tag_color`, `delete_tag`, `assign_tag`, `unassign_tag` with trimmed NOCASE validation, verifying duplicate/empty-name cases are rejected
- [x] 2.2 Extend `api_get_meetings` response to include `created_at` + `tags[]` ordered by `created_at DESC`, verifying payload shape via invoke smoke test
- [x] 2.3 Register new commands in `lib.rs` and verify `cargo check -p meetily` passes

## 3. Sidebar list display (date + compact type + pills)

- [x] 3.1 Plumb `created_at` + `tags` through `SidebarProvider fetchMeetings` into `sidebarItems`, verifying rows receive the new fields without breaking navigation
- [x] 3.2 Rework `renderItem` row to title-first layout: full-width title wrapping up to 3 lines (`line-clamp-3`, compact type), meta line with `yyyy-mm-dd hh:mm` local date (`formatMeetingDate` helper, `--` fallback) left and action buttons right, verifying midnight/noon render as `00:05`/`12:00` and buttons never overlap the title
- [x] 3.3 Add tag pills with palette colors, visible cap + `+M` overflow and tooltip, verifying many-tag, few-tag, and zero-tag rows all keep row height stable

## 4. Tag editor and search

- [x] 4.1 Move tag popover trigger plus rename/delete buttons to the date/tags meta line (title line button-free), keeping autocomplete from `list_tags`, create-and-assign on Enter, assign/unassign toggle, stopPropagation, verifying create/assign/unassign round-trips update the row
- [x] 4.2 Extend `filteredSidebarItems` search to match tag names additively, verifying tag-only matches appear while title/transcript matches still highlight as before
- [x] 4.3 Reuse tag editor/display on meeting-details header if space allows (or leave display-only), verifying no duplicate fetch loops

## 5. Verification

- [x] 5.1 Run `cargo test` for database/tag paths and `pnpm -C frontend lint` (or `tsc --noEmit`), verifying zero new errors
- [ ] 5.2 Manual pass: create/rename/recolor/delete tags, reload app, delete a tagged meeting, verifying persistence, cascade, list rendering, title wrap up to 3 lines, and action buttons on the meta line end to end
