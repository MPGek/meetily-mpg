## Context

See `proposal.md` for motivation. Current state (verified in repo during exploration):

- `meetings.created_at` is written as `Utc::now()` at save time in both creation paths — `TranscriptsRepository::save_transcript` (`database/repositories/transcript.rs:24-28`) and the audio import insert (`audio/import.rs:694-717`). The row is created on stop/import, so `created_at` is effectively the stop time.
- The true start already exists on disk: `RecordingSaver` writes `metadata.json` with `created_at = Utc::now().to_rfc3339()` when recording begins (`audio/recording_saver.rs:291-311`), plus `completed_at`/`duration_seconds` on stop. It never reaches the DB.
- The stop flow (`api_save_transcript`, `api/api.rs:1089`) receives `folder_path`, so the backend can read the folder's `metadata.json` at save time. A metadata read/write helper already exists (`summary/metadata.rs`, `METADATA_FILE = "metadata.json"`).
- Live session memory (`ONLINE_SESSION_DATA` in `recording_commands.rs`) holds per-recording state until `finalize_online_session`, but is lost on crash — unsuitable for start time, which must survive the crash-recovery path.
- Consumers: `api_get_meetings` → `Meeting { id, title, created_at, tags }`; frontend formats via `formatMeetingDate` (`frontend/src/lib/meeting-tags.ts`). List ordering is `created_at DESC` (spec `database`).

## Goals / Non-Goals

**Goals:**

- Persist start time for both creation paths with documented fallbacks; expose it in the list API; display prefers it with `created_at` fallback.
- Survive crash-recovery (source must be on disk, not in memory).

**Non-Goals:**

- No change to list ordering (`created_at DESC` stays; avoids touching the `database` ordering requirement).
- No duration display (`completed_at`/`duration_seconds` stay folder-only; natural follow-up).
- No `created_at` reinterpretation (old rows keep stop-time values honestly; see Risks).

## Decisions

### D1: New nullable `started_at` column over reinterpreting `created_at`

Rewriting `created_at` at save needs no migration, but leaves mixed semantics in one column (pre-feature rows = stop, new rows = start) with no way to distinguish them, and destroys stop time. A nullable `started_at` keeps both facts, backfills honestly (`= created_at`), and every reader degrades via `?? created_at`.
Alternative (stop − duration estimation for old rows) rejected: approximate, heavier migration, marginal value.

### D2: Read start from `metadata.json` at save time, not from session memory

At `save_transcript`, if `folder_path` is present, read the folder's `metadata.json created_at`, parse RFC3339, use as `started_at`; on any failure use `now()`. The file is written at recording start, survives crashes, and needs no changes to the start flow or stop-flow plumbing (no new IPC args, no frontend stop changes).
Alternative (capture `Utc::now()` in session state at `start_recording`, thread through to stop) rejected: lost on crash, duplicates data already on disk; session memory is reserved for non-critical hints (cf. expected-speaker allowlist with match-all fallback).
Alternative (frontend sends start time at stop) rejected: client clock vs backend clock skew, larger IPC change for zero benefit.

### D3: Import uses audio file mtime

No recording start exists for imports; file mtime is the closest observable proxy for «when this was recorded». Fallback chain: `mtime → now()`. Edited files may lie — accepted and documented (spec scenario covers fallback only; mtime semantics are best-effort by nature).

### D4: Display rule `started_at ?? created_at`, ordering untouched

`Meeting` DTO gains `started_at: Option<String>` (RFC3339); `MeetingModel` gains the `#[sqlx(default)]` field. Frontend `formatMeetingDate` call sites pass `started_at ?? created_at` (sidebar first; other surfaces as encountered). Ordering stays `created_at DESC` — for same-day recordings start/stop orders coincide except pathological overlaps; changing ordering would modify the `database` spec requirement for no user-visible gain in v1.

## Risks / Trade-offs

- [Unparsable `metadata.json`] → Fallback `now()`; save never fails because of start-time resolution (spec mandates).
- [Legacy rows show stop time] → Documented in spec + migration comment; `started_at = created_at` backfill is explicit, not silent.
- [Import mtime lies for edited files] → Accepted best-effort; import moment would lie more.
- [Two timestamps confuse future readers] → `started_at` = «when recording began», `created_at` = «when the DB row was created»; migration + model comments state this.
- [Recovery path bypasses `save_transcript`] → Verify during implementation which save path recovery uses; if it writes its own INSERT, apply the same metadata.json read there (task covers it).

## Migration Plan

1. SQLx migration `..._add_meeting_started_at.sql`: `ADD COLUMN started_at TEXT`, backfill `UPDATE meetings SET started_at = created_at WHERE started_at IS NULL`. Forward-only; old builds ignore the column.
2. Backend: `MeetingModel.started_at`, both insert paths, list DTO field.
3. Frontend: optional `started_at` in meeting types, `?? created_at` at display sites.
4. Rollback: revert code; column stays inert; no data loss.

## Open Questions

- None blocking. Possible follow-ups (duration display, ordering by `started_at`) intentionally deferred — see Non-Goals.
