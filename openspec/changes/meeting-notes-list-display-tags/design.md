## Context

See `proposal.md` for motivation. Current state (verified in repo):

- List UI: `frontend/src/components/Sidebar/index.tsx:554 renderItem` renders one-line `text-sm` title rows in a `w-64` sidebar; no date, no tags. See `specs/meeting-list-display/spec.md`.
- Data flow: `MeetingsRepository::get_meetings` (`frontend/src-tauri/src/database/repositories/meeting.rs:18`) returns full `MeetingModel` ordered by `created_at DESC`, but `SidebarProvider.tsx:89 fetchMeetings` narrows to `{id, title}`, dropping `created_at`. `created_at` is RFC3339 via `DateTimeUtc` (`database/models.rs:6`).
- Tags: no storage, no commands, no UI. `/notes/[id]/page.tsx` tag rendering is a dead static mock, unrelated to real meetings (`/meeting-details?id=`).
- Search: `searchTranscripts` + `filteredSidebarItems` (`Sidebar/index.tsx:259`) matches title/transcript; tag matching is new.

## Goals / Non-Goals

**Goals:**

- Enriched list rows (date + compact type + tag pills) with no sidebar widening.
- Normalized tag dictionary + link table with new Tauri commands.
- `api_get_meetings` returns `created_at` + tags; delete path cascades links.

**Non-Goals:**

- No separate registry/table page, no sorting UI beyond existing `created_at DESC`, no bulk tag operations, no tag import/export.
- No dark-mode palette pass (keep pairs readable on light theme; note as follow-up).
- No changes to recording, transcription, diarization, summary pipelines.

## Decisions

### D1: Enriched list rows, not a table

Sidebar is 256px; a table needs 600px+ for aligned columns. A table would force a new page/route and dual list maintenance.

- Chosen: title-first row — line 1 title full width (`text-xs`, `line-clamp-3`, `break-words`, `title` attr), line 2 meta row with date left (`text-[11px] text-gray-500 tabular-nums`) and action buttons right (tag editor trigger, rename, delete — hover/focus reveal), line 3 tag pills (`text-[10px]`, cap 2 + `+M`). Explicit trade-off: rows are taller so fewer fit per viewport, but titles like `Встреча по...` stay readable; the single-line-truncate-for-density alternative was rejected on user feedback.
- Alternative (dedicated `/meetings` table page) rejected for this change: valuable later for filtering 50+ meetings, but out of scope.
- Alternative (widen to `w-80`) rejected: steals transcript width; readability gain comes from wrapping, not width.

### D2: Normalized `meeting_tags` + `meeting_tag_links` over JSON column

JSON in `meetings.tags` is one column but makes "pick existing", rename, usage counts, and duplicate prevention client-side string work.

- Chosen schema: `meeting_tags(id TEXT PK, name TEXT, color TEXT, created_at, updated_at)` + `meeting_tag_links(meeting_id, tag_id PK)`, `UNIQUE(name COLLATE NOCASE)`, FKs to `meetings(id)` / `meeting_tags(id)`, index on `links(meeting_id)` and `links(tag_id)`. Name stored trimmed; comparison NOCASE (same pattern as `speakers.name` unique-nocase index).
- `delete_meeting_with_transaction` gains `DELETE FROM meeting_tag_links WHERE meeting_id = ?` before meeting delete (FK enforcement is off at runtime — manual cascade like speaker tables).
- Alternative (JSON) rejected: rename becomes full-table rewrite, autocomplete needs DISTINCT+parse.

### D3: Deterministic default color, stored, overridable

- Fixed palette of ~10 Tailwind pairs (e.g. blue/green/purple/amber/rose/cyan/lime/orange/teal/fuchsia as `bg-*-100/text-*-800/border-*-200`). Default = `palette[stable_hash(lower(name)) % len]` so `Work` is always the same color on fresh DBs without coordination.
- `color` column stores the resolved pair key (e.g. `blue`), not raw CSS, so a later palette tweak re-skins consistently; manual color change = UPDATE row.
- Alternative (assign by creation order) rejected: order-dependent, non-deterministic across devices.
- Alternative (free hex picker) rejected for v1: contrast validation burden; palette keys keep a11y bounded.

### D4: Extend `api_get_meetings` payload, batch tag load

- `MeetingModel` gains optional `tags: Vec<TagDTO>` only on the list DTO (not the `FromRow` struct) to avoid N+1 mapping complexity in `query_as`. Implementation: one `SELECT * FROM meetings ORDER BY created_at DESC`, one `SELECT l.meeting_id, t.id, t.name, t.color FROM meeting_tag_links l JOIN meeting_tags t ... WHERE l.meeting_id IN (...)`, group in Rust into `MeetingWithTags { id, title, created_at, updated_at, tags }`.
- `created_at` serialized as existing RFC3339; frontend formats to `yyyy-mm-dd hh:mm` local via a small `formatMeetingDate` helper (guard `Invalid Date` → placeholder `--`).
- Alternative (per-meeting tag query) rejected: N+1 on 100 meetings.
- Alternative (separate `list_meeting_tags` per row on expand) rejected: list needs tags eagerly for pills.

### D5: Tag editor as popover in sidebar row + full control on meeting page (display first)

- Sidebar row: the tag-editor trigger (tag icon) sits on the meta line next to rename/delete — hover/focus reveal, search input with autocomplete from `list_tags`, click toggles assign/unassign, Enter creates-and-assigns, small `x` on assigned pills unassigns. Stop propagation so row navigation doesn't fire. The title line carries no buttons.
- Meeting-details page: same component reused near title (space allows full wrap, no `+M` cap needed there).
- New commands: `list_tags` (with `usage_count`), `create_tag(name, color?)`, `rename_tag`, `set_tag_color`, `delete_tag`, `assign_tag`, `unassign_tag`. Frontend invalidates both `meetings` and tag list after each mutation; optimistic toggle with revert on error.

### D6: Search matches tags additively

- `filteredSidebarItems` gains: `tagNames = meeting.tags.map(lower)`; match if `query in tagNames` OR existing title/transcript match. No ranking change; transcript snippet highlight path untouched.

## Risks / Trade-offs

- [NOCASE duplicates with Unicode] → Trim + NOCASE unique index; residual homoglyph dupes accepted for v1, surfaced via autocomplete ordering by usage.
- [Taller rows fit fewer per viewport] → Accepted: wrapping to 3 lines trades density for readability; compact type (`text-xs`/`11px`) and capped pills bound the growth.
- [Popover in 256px overflows] → Radix/shadcn popover with `align=start`, `sideOffset`, max-h + scroll; tag cap 2 keeps row height stable.
- [Palette contrast on colored pills] → Fixed vetted pairs only, no free hex in v1.
- [Timezone confusion] → Format in local tz explicitly; tooltip shows full ISO with offset.
- [Migration on large DBs] → Two tiny tables + indexes; no backfill; rollback = drop tables (no data loss to meetings).

## Migration Plan

1. SQLx migration `YYYYMMDDHHMMSS_add_meeting_tags.sql` (create tables, indexes) — forward-only, backwards compatible (old code ignores new tables).
2. Backend: models + repository + commands + `lib.rs` registration.
3. Frontend: provider DTO, row layout, pills, popover editor, search match.
4. Rollback: revert frontend/backend; tables stay inert; meetings data untouched.

## Open Questions

- Visible tag cap: 2 vs 3 in sidebar row — verify visually during implementation, spec allows either via "visible cap".
- Whether meeting-details header also gets the editor in v1 or display-only — approach supports both; decide at build time without spec change.
