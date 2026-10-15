## Context

See `proposal.md` for motivation. Current state (verified in repo):

- Recording starts in one click from Home (`useRecordingStart.handleRecordingStart` → `recordingService.startRecordingWithDevices`); there is no pre-recording interstitial. Start paths without UI exist too (tray, `autoStartRecording` flag from sidebar, `start-recording-from-sidebar` event).
- The meeting DB row is created only at stop (`api_save_transcript` via `storageService.saveMeeting`, which passes `folder_path`), so pre-start tags have nothing to link to until then.
- Established precedent for session-scoped data: `metadata.json` in the meeting folder already carries `created_at` (written at start, read nowhere in DB flow yet) and the expected-speaker allowlist is held in session memory until `finalize_online_session` persists it. A metadata read/write helper exists (`summary/metadata.rs`).
- Tag primitives exist from the tags change: `list_tags`, `create_tag`/`create_and_assign_tag`, `assign_tag`/`unassign_tag`, `TagEditorPopover` patterns, palette in `frontend/src/lib/meeting-tags.ts`.

## Goals / Non-Goals

**Goals:**

- One pending-tag concept editable before and during recording, linked atomically-ish at save.
- Survive frontend reload mid-recording and crash-recovery (same bar as the audio itself).
- No DB migration.

**Non-Goals:**

- No per-template/default tag sets, no «remember last selection» (follow-up).
- No import-dialog tagging (import has no before/during phase; out of scope).
- No changes to sidebar/meeting-details tag editing.

## Decisions

### D1: Pending set persisted in `metadata.json`, not (only) in React state

The pending set (tag ids) is written to the meeting folder's `metadata.json` under a new optional key whenever it changes, and read back at save time for linking. Rationale: it reuses the exact precedent of `metadata.json created_at` (crash-safe, reload-safe, no new IPC plumbing for the bytes themselves), and crash-recovered recordings keep their tags just like they keep their start time.
Alternatives: pure React context — lost on reload/crash of the UI, exactly when tagging context matters most; backend session memory (`ONLINE_SESSION_DATA` style) — lost on backend-process crash while `metadata.json` survives; both rejected as strictly weaker with no compensating simplicity once the metadata helper exists.

### D2: One editor component, two mount points, backend as source of truth

A single pending-set editor (built on the `TagEditorPopover` interaction pattern: autocomplete from `list_tags`, Enter-to-create, toggle) mounts (a) on Home next to Start Recording pre-start and (b) in the recording panel during recording. Frontend mirrors the set locally for responsiveness and syncs each change to the backend (`set_recording_tags`-style command writing `metadata.json`); on mount during an active recording it loads from the backend. New tags are created in the dictionary immediately via `create_tag`, so the pending set always stores ids (rename-safe).
Alternative (two separate states for before/during) rejected: spec requires one set; two states invite divergence bugs at the start transition.

### D3: Link at save, best-effort per tag, meeting save never fails on tags

At stop, after the meeting row exists, the saver links each pending id via the existing link path, collecting per-tag errors into a user-visible warning (toast) instead of failing the save. Unknown/stale ids (tag deleted mid-recording) are skipped with the same warning. Rationale: tags are annotation, the recording is the asset — priority order per spec.
Alternative (link inside the save transaction, all-or-nothing) rejected: a tag failure must not endanger the meeting.

### D4: Starts without UI begin empty; cancel clears

Tray/auto starts never pass through the picker, so the backend initializes the pending key to `[]` at recording setup; cancel/discard clears it (and a fresh start re-initializes). Pre-created dictionary tags intentionally survive cancellation (zero-usage rows, same as creating a tag anywhere and never using it).

## Risks / Trade-offs

- [Stale ids after mid-recording tag delete] → Skip + warn at link time; ids (not names) make renames safe.
- [Two writers (pre-start UI, in-recording UI)] → Single backend key + load-on-mount; last write wins, no merge needed (one user, one recording).
- [`metadata.json` write races with saver] → Helper does atomic temp-rename writes (existing pattern); pending-key updates are tiny and infrequent.
- [Old builds ignore the new key] → Optional JSON key, forward/backward compatible; no migration.

## Migration Plan

No DB migration. Deploy backend (new commands + save-time linking) with frontend (picker + panel editor); either side alone is inert (no pending key → today's behavior). Rollback: revert; `metadata.json` key ignored.

## Open Questions

- None blocking. Exact Home placement (inline row vs collapsible section) is a build-time visual call within the spec.
