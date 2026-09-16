## Context

See `proposal.md` - Why. Read-only evidence gathered on this machine at planning time:

- Every recording folder under `~/Music/meetily-recordings/Meeting 2026-09-1*` contains `"pending_tag_ids": []`, i.e. the value `RecordingSaver::initialize_meeting_folder` writes at start; no folder has a non-empty set.
- The app database (`%APPDATA%/com.meetily.ai/meeting_minutes.sqlite`) has 7 `meeting_tag_links` rows, all for meetings created 2026-09-09..2026-09-14; none of the five 2026-09-15 recordings has a link.
- Those same 2026-09-15 meetings have a populated `started_at`, which is read from the same `metadata.json` at save time. So a valid `folder_path` reached `api_save_transcript`; the linking path ran and read an empty pending set.

Current behavior already in place (HEAD): the pending key is written at recording setup; `RecordingSaver::write_metadata` read-modify-merges unmodeled keys under a shared lock; the pre-start set is pushed on the start transition with two retries; mid-recording writes are serialized. The built frontend bundle carries that push code. The remaining failures are therefore the *silent* ones: a write rejected because the recording folder is not resolvable yet, a start-time flush whose retries are exhausted, a save-time read that resolves no folder, and a `load()` that replaces a non-empty local selection with an empty backend read. None of these produces a user-visible error.

## Goals / Non-Goals

**Goals:**

- A tag selected before start or during recording is linked to the saved meeting, or the user is told why it was not.
- No code path clears the visible pending set without user intent (cancel, discard, or save).
- Save-time linking does not depend on frontend session state alone.
- Palette of at least 30 distinct colors with backend/frontend key parity.

**Non-Goals:**

- Per-template or remembered default tag sets.
- Tagging in the import dialog.
- Moving the pending set out of `metadata.json` into the database (no migration).
- Replacing palette keys with raw CSS colors.

## Decisions

### D1: Keep `metadata.json` as the pending-set store

The pending key stays in the recording folder's `metadata.json`, reusing the crash-safe precedent of `created_at`. Alternative: a session table in SQLite - rejected, it needs a migration and gives no stronger crash-safety than the file that already survives process death.

### D2: A start transition confirms the pushed set; it does not re-read and overwrite

On the `isRecording` false->true transition the hook must push the local pre-start selection and trust the write's returned canonical list, rather than calling `load()` afterwards and replacing local state with whatever the backend returns. `load()` remains only for mount and reload. Alternative: push then read - rejected, that is exactly the racy overwrite seen in practice.

### D3: An empty read is not "user cleared" while recording

A failed or transient read MUST preserve the current selection. Only an explicit user action, a cancel/discard, or a completed save clears the visible set. Alternative: treat any empty read as authoritative - rejected; it converts a transient failure into silent data loss.

### D4: Save-time linking resolves the folder server-side

The backend already knows the finished recording's folder when the saver stops. Keep that path available so `api_save_transcript` can link pending tags even when the frontend passes no `folder_path`, and return a warning in the save response when a non-empty pending set cannot be read. Alternative: continue relying on the frontend's stored folder path - rejected; a missing value currently yields no links and no warning.

### D5: Palette stays key-based and is expanded to 40 keys

Backend `MEETING_TAG_PALETTE` and frontend `TAG_PILL_STYLES` grow to the same 40 keys; the deterministic FNV assignment is unchanged and `nextColor` already cycles the full list. All chip classes stay literal strings so Tailwind keeps them. Alternative: assign colors by a sequential counter - rejected because it breaks the "same tag name always resolves to the same default color" scenario.

### D6: Palette parity is enforced by a test on each side

Add a backend test and a frontend test asserting the palette key set and count, so the two copies cannot drift silently. Alternative: generate the TypeScript list from Rust at build time - rejected as new tooling for a small fixed list.

## Risks / Trade-offs

- [Tailwind never scanned `src/lib`, so the palette classes in `meeting-tags.ts` were purged and the pills rendered with no background] -> The effective `tailwind.config.js` content globs now include `./src/lib/**`, and compilation emits every palette class (`bg-lime-100`, `bg-stone-100`, `text-zinc-800`, ...). `tailwind.config.ts` is an unused duplicate and stays untouched.
- [A stale dev or backend binary hides the fix during verification] -> The manual pass must rebuild the Tauri backend and hard-reload the frontend first; the acceptance evidence is on disk (non-empty `pending_tag_ids` during recording, links present after save).
- [40 hues still look similar under some monitors] -> Vary both hue and shade across the palette; accept approximate, not perfect, distinctness.
- [Palette key drift between Rust and TypeScript] -> D6 test plus keeping both lists in one obvious place.
- [Concurrent `metadata.json` writers] -> Keep all writes under the existing shared `METADATA_WRITE_LOCK` with atomic temp-rename.

## Migration Plan

No database migration. Deploy backend and frontend together; either side alone is inert. Rollback: revert the change; the optional `metadata.json` key and the existing palette keys remain valid.
