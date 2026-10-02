# Proposal

## Why

A summary run can publish wrong or incomplete results, and the run's own state is not trustworthy. Five problems, all in the fork today:

- **Reasoning models break.** Ollama reasoning models (qwen3, deepseek-r1, gpt-oss) can return `"content": null` with the text in `reasoning`/`reasoning_content`. `MessageContent.content` is a plain `String` (`summary/llm_client.rs:42`), so the response fails to parse and the run fails.
- **Chunks are silently dropped.** `generate_meeting_summary` pushes each chunk's raw output, think tags included, straight into the combine prompt (`summary/processor.rs:435`). A chunk that fails is logged and skipped (`processor.rs:443`), so a meeting with one failed section produces a summary that looks complete but is missing part of the meeting.
- **Cleaning is weak.** The think-tag regex is case-sensitive and does not allow attributes (`processor.rs:11-12`). An unclosed `<think>` or reasoning-only output passes through: either reasoning reaches the saved notes, or an empty summary is saved as `completed`.
- **Runs overwrite each other.** Terminal writes match only `WHERE meeting_id = ?` (`database/repositories/summary.rs:136,173,207`). The cancellation registry is keyed only by meeting (`summary/service.rs:28-30,197-229`). So a run that was superseded or cancelled can still write `completed`/`failed` over the newer run, and when the old run cleans up it removes the newer run's cancellation token. `api_process_transcript` leaves the row `PENDING` forever if saving the transcript fails (`summary/commands.rs:378`, `?` after `create_or_reset_process` at 368), and so does an app exit during a run.
- **Progress is lost on navigation.** Leaving a meeting stops polling (`app/meeting-details/page.tsx:189-197`), and `useSummaryGeneration` never resumes on mount. Coming back shows an idle page while the backend is still generating, and auto-summary can start a duplicate run. `SidebarProvider` polling (`components/Sidebar/SidebarProvider.tsx:77,193-293`) keeps a state `Map<string, Timeout>` with no in-flight guard and no run-identity check. Its cleanup effect depends on that map, so finishing one meeting's poll clears every other meeting's interval.

Upstream v0.4.1 fixed these in #744 (with #665) and #779. We re-implement the ideas on the fork's architecture: the shared `crate::llm::send_with_retry` and `LlmError`, typed IPC under `frontend/src/lib/ipc/`, the fork's summary cache and language detection, and the split meeting-details components. We do not port the upstream code itself.

## What Changes

- **Ollama compatibility.** OpenAI-compatible requests to Ollama send `reasoning_effort: "none"`. If Ollama rejects the field (an `LlmError::Http` with status 400/422 whose body mentions it), the request is sent again once without it. The response message's `content` becomes optional, and `reasoning` / `reasoning_content` are parsed but never used as summary text.
- **Strict output cleaning on every LLM stage** (chunk, combine, final, translation, normalization):
  - Reasoning envelopes are matched case-insensitively and may carry attributes.
  - Output that still has an unclosed or stray reasoning marker after cleaning is a failure.
  - Output that is empty after cleaning is a failure.
  - Whether reasoning was stripped is logged once per run. It is not persisted (see design.md D4).
- **Chunk failure semantics (BREAKING, behavioral).**
  - Each chunk is cleaned before it goes into the combine prompt.
  - A chunk that fails is retried once.
  - If the retry also fails, the whole run fails with a user-visible error naming the section, and the previous summary is restored. A partial summary is never published.
- **Run-scoped state.**
  - Each run is identified by its `summary_processes.start_time`. This column already exists, so no migration is needed.
  - `completed`/`failed`/`cancelled` writes are compare-and-set on `(meeting_id, start_time, status = pending)` and report whether they applied.
  - Cancellation tokens are keyed by run, and starting a new run cancels the previous run for that meeting.
  - The token is registered before the background task is spawned, so an early Stop is not lost.
  - A failure to save the transcript marks the run failed.
  - At startup, rows left `pending` by a previous process are marked failed as interrupted.
  - The meeting rename from the summary title happens only when this run's completion write applies.
- **IPC.** `api_process_transcript` returns the run id (`start_time`, RFC 3339 with nanoseconds) as `process_id`, and `api_get_summary.start` uses the same format. `api_cancel_summary` takes a required `processId` and cancels only that run. Only `useSummaryGeneration` calls this, through `lib/ipc/summary.ts`.
- **Frontend progress.**
  - The meeting page keeps the stored summary response and hands it to `useSummaryGeneration`.
  - On mount, a `pending` run resumes as processing (or regenerating, when an older summary exists) and polling restarts for that run id.
  - A run that failed while the meeting was closed shows its stored error, with no toast.
  - Auto-summary starts only when the stored status is `idle`.
  - A late result for a previous meeting, or for an older run, is ignored.
  - `SidebarProvider` polling moves to a ref-held registry with an in-flight guard and run-identity checks, and a stop only takes effect when it names the active run.
- Adapt upstream's `frontend/tests/hooks/summary-generation.test.tsx` to the fork's IPC and hook surface. This adds `react-test-renderer` as a dev dependency.

## Capabilities

### New Capabilities
- `summary-progress-tracking`: how the meeting view mirrors the stored state of the meeting's summary run. It covers resuming an in-progress run on return, showing a failure that happened while the meeting was closed, gating auto-summary on stored idle state, and ignoring results from a previous meeting or an older run.

### Modified Capabilities
- `summary-service`:
  - "Transcript chunking for large inputs" now cleans each chunk, retries a failed chunk once, and fails the whole run instead of publishing a partial result.
  - "Summary cancellation support" is now run-scoped.
  - New requirements cover reasoning-free visible output and the Ollama reasoning compatibility fallback.
- `database`: "Summary process tracking" now has run-scoped compare-and-set terminal writes and fails interrupted `pending` rows at startup.
- `llm-provider-resilience`: "Authentication and validation failures are not retried" gains one explicit exception. A single Ollama compatibility re-send with `reasoning_effort` removed is a different request, not a transport retry.

## Impact

- **Backend:**
  - `frontend/src-tauri/src/summary/llm_client.rs`: request body, response structs, compatibility re-send.
  - `summary/processor.rs`: regexes, stage cleaning, chunk loop, reasoning flag.
  - `summary/service.rs`: run-keyed registry, CAS outcome handling, rename ordering.
  - `summary/commands.rs`: run id, token registration before spawn, transcript-save failure, `api_cancel_summary(process_id)`, `start` format.
  - `database/repositories/summary.rs`: `started_at` parameter, CAS updates returning `bool`, `fail_interrupted_runs`.
  - `database/setup.rs`: call `fail_interrupted_runs` once at startup.
- **Frontend:**
  - `frontend/src/lib/ipc/summary.ts`: `CancelSummaryArgs` with `processId`.
  - `components/Sidebar/SidebarProvider.tsx`: ref-based polling. `activeSummaryPolls` leaves the context because no consumer reads it.
  - `hooks/meeting-details/useSummaryGeneration.ts`: `initialSummary`, resume, run-identity guards.
  - `app/meeting-details/page.tsx`: stored summary response, a fetch guard against stale meetings, idle-gated auto-summary, and the polling-stop cleanup moves into the hook.
  - `app/meeting-details/page-content.tsx`: passes `initialSummary` through and gates auto-generation on `summaryStatus === 'idle'`.
- **Tests:**
  - New Rust unit tests in `llm_client.rs` (wiremock), `processor.rs`, `service.rs`, and `repositories/summary.rs` (in-memory SQLite with real migrations).
  - New `frontend/tests/hooks/summary-generation.test.tsx`.
  - New dev dependencies `react-test-renderer@18.3.1` and `@types/react-test-renderer@18.3.1`, which change `pnpm-lock.yaml`.
- **No DB migration.** `summary_processes.start_time TEXT` already exists (`migrations/20250916100000_initial_schema.sql`).
- **Depends on `port-upstream-041-quick-fixes`, which must land first.** It owns #603 (chunk-boundary coverage in `chunk_text`) and #694 (Claude thinking-block parsing in `llm_client.rs`). This change edits the same two files and builds on that change's Claude response parsing. It does not touch `chunk_text` or Claude text-block selection.
- **Out of scope:**
  - Upstream's `MeetingDetailsSplitView`, Tailwind/PostCSS cleanup, `Logo`, and container-query UI.
  - Upstream's prompt wording changes ("do not include reasoning").
  - Upstream's `api_save_meeting_summary` renderability validation and its read-time redaction of stored reasoning.
  - Making the response-body read cancellable.
  - The frontend's 200-poll (~16.5 min) polling timeout. It is unchanged; see design.md Risks.
