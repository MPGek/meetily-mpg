# Design

## Context

See proposal.md (Why) for the motivation and specs/ for the required behavior. This section covers only the current state and the constraints that shape the approach.

- **Transport (change 10).** `generate_summary` (`summary/llm_client.rs:114-394`) sends every request through `crate::llm::send_with_retry` (`llm/client.rs:89-147`). That layer retries connect errors, timeouts, 429 and 5xx up to 2 more times. It returns any other 4xx at once as `LlmError::Http { status, body }`, with the response body already read. The `tokio::select!` against the cancellation token (`llm_client.rs:305-330`) wraps the send future but not the body read that follows (`.json()` at 336/352).
- **Request body.** It is built as a `serde_json::Value` from `ChatRequest` (`llm_client.rs:17-27, 230-265`). The response is parsed into `ChatResponse` → `Choice` → `MessageContent { content: String }` (`llm_client.rs:30-43`).
- **Processor.** `generate_meeting_summary` (`summary/processor.rs:327-604`) runs these stages: optional per-chunk passes (`400-446`), an optional combine pass (`462-489`), the final report (`521-540`), then translation or English normalization through `run_markdown_transform` (`607-650`). Cleaning (`clean_llm_markdown_output`, `264-284`, re-exported at `summary/mod.rs:77`) is applied only to the final report and the transforms. It never fails.
- **Run row and service.** `summary_processes` already has `start_time TEXT`, and `create_or_reset_process` writes it (`database/repositories/summary.rs:85-119`, `now` taken inside). sqlx encodes `DateTime<Utc>` as a fixed RFC 3339 text with full sub-second precision, so binding the same value again matches the stored text exactly. `api_process_transcript` creates the row, saves the transcript, then spawns `SummaryService::process_transcript_background` (`summary/commands.rs:368-406`). The background task, not the command, registers the cancellation token (`service.rs:317`).
- **Frontend.** The only caller of `cancelSummary` and `startSummaryPolling` is `useSummaryGeneration` (`hooks/meeting-details/useSummaryGeneration.ts:168, 619`). `activeSummaryPolls` is exposed in the sidebar context but read nowhere else. `PageContent` unmounts when the meeting changes, because `page.tsx` resets `meetingDetails` to `null` and renders a spinner, so the hook's state already resets per meeting.
- **Test infrastructure.** Repository tests use in-memory SQLite with the real migrations (e.g. `database/repositories/meeting.rs:450`). `wiremock` is already a dev-dependency. Frontend tests run under `bun test`, where `mock.module` is process-global.

## Goals / Non-Goals

**Goals:**
- Use one identity for a run (its start time) throughout: the DB compare-and-set, the cancellation registry, the IPC `process_id`/`start`, and the frontend poll guard.
- Keep `generate_summary`'s signature (`Result<String, String>`) unchanged. The sibling `port-upstream-041-quick-fixes` edits its Claude branch, and the processor calls it from six places.
- Make each of task groups 1-4 a self-contained, green commit.

**Non-Goals:**
- Making the response-body read cancellable (upstream's `await_or_cancel`). With non-streaming Ollama the body arrives together with the headers, after generation, so the uncovered window is negligible.
- Changing the transport retry policy, or skipping the chunk retry for errors the transport layer has already retried.
- Persisting `reasoning_stripped` or surfacing it in the UI (D4).
- Fixing the race where a user edit (`update_meeting_summary`) and a running regeneration both write the row. That is a different problem.
- Changing the frontend's 200-poll timeout (Risks).

## Decisions

### D1: The run id is `summary_processes.start_time`, created under the registry lock

`SummaryService::register_run(meeting_id) -> (DateTime<Utc>, CancellationToken)` takes the registry mutex and sets `started_at = Utc::now()`. If an entry for the meeting exists and its `started_at >= now`, it uses that value plus 1 ns instead. It then cancels the previous entry's token and inserts the new `{ started_at, token }`. This guarantees that a meeting's concurrent runs never share a start time, which would make the compare-and-set ambiguous. A superseded run is cancelled so it stops calling the LLM, and its later writes are rejected anyway (D3).

The wire form is a single helper, `run_id(&DateTime<Utc>) -> String`, defined as `to_rfc3339_opts(SecondsFormat::Nanos, true)`. It is used for the `process_id` returned by `api_process_transcript` and for `api_get_summary`'s `start` (`commands.rs:284`, currently `to_rfc3339()`). `api_cancel_summary` parses `process_id` back with `DateTime::parse_from_rfc3339`.

Alternatives:
- **A new `run_id` UUID column.** Needs a migration and a second identity next to `start_time`. Rejected.
- **Upstream's global `SUMMARY_START_LOCK` plus a static last-start value.** Same guarantee with two extra statics. The registry mutex already serializes starts per meeting. Rejected.

### D2: Register before the row exists, and hand the token to the background task

`api_process_transcript` order:
1. `register_run`.
2. `create_or_reset_process(pool, id, started_at)`. On error, remove the registration and return the error.
3. `save_transcript_data`. On error, compare-and-set the run to failed with the error message, remove the registration, and return the error. Today that path leaves the row `PENDING` forever.
4. Spawn `process_transcript_background(..., started_at, token)`.

The background task no longer registers anything itself. Every `update_process_failed` call in it passes `started_at`. Cleanup at the end runs `cleanup_run(meeting_id, started_at)`, which removes the entry only if `started_at` still matches. This fixes the bug where an old run's cleanup removed a newer run's token.

`cancel_summary(meeting_id, started_at) -> bool` cancels only an entry whose start matches.

Because registration now happens before spawn, a Stop that arrives right after the start reaches the run. Today it returns "No active summary generation" while the run keeps going.

### D3: Terminal writes are compare-and-set and report whether they applied

`update_process_completed`, `update_process_failed` and `update_process_cancelled` gain `started_at: DateTime<Utc>`. Their `WHERE` becomes `meeting_id = ? AND start_time = ? AND LOWER(status) = 'pending'`, and they return `Result<bool, sqlx::Error>` (`rows_affected() == 1`).

The service logs `Ok(false)` as `warn!("Skipped stale summary {completed|failed|cancelled} …")`.

The meeting rename from the summary title (`service.rs:571-582`) moves after the completed write and runs only on `Ok(true)`. Today a stale run can rename the meeting.

`api_cancel_summary` keeps writing `cancelled` itself, so the UI updates at once. The background task's own cancelled write then becomes a no-op. The `pending` condition is what makes "first terminal write wins" hold.

The response JSON keeps its current `{ message, meeting_id }` shape. The message distinguishes three cases: cancelled, run already finished, and no matching active run.

Alternatives:
- **Matching on start time without the status condition.** Lets a late `completed` overwrite a `cancelled`. Rejected.
- **Version-number column.** Needs a migration. Rejected.

### D4: `reasoning_stripped` is logged, not persisted

The processor's stage cleaner (D6) reports whether it removed a reasoning envelope. `generate_meeting_summary` ORs that across stages and logs it once, in its final `info!` line (`processor.rs:602`). Its return type is unchanged.

`generate_summary` logs at `info!` when a response carried a non-empty `reasoning`/`reasoning_content` field that was discarded. It reports this as a length, never the text.

Nothing is added to the result JSON or the IPC types. Upstream persists the flag only to choose a toast wording. Nothing in the fork reads it, and the raw output (think tags included) is already preserved in the per-call debug logs (`llm-debug-logging`) for diagnosis.

Alternative: persist it in the result JSON like upstream. That adds a JSON field, a TS type field, and read-side handling with no consumer. Rejected as speculative.

### D5: Ollama compatibility lives in `generate_summary`, keyed off `LlmError::Http`

- `ChatRequest` gets `#[serde(skip_serializing_if = "Option::is_none")] reasoning_effort: Option<&'static str>`, set to `Some("none")` only when `provider == Ollama`.
- `MessageContent` becomes `{ content: Option<String>, reasoning: Option<String>, reasoning_content: Option<String> }`, all `#[serde(default)]`. The visible text is `content.unwrap_or_default().trim()`.
- The send is wrapped in one async block, still inside the existing `select!` against cancellation:
  1. Run `send_with_retry` with the body.
  2. If the result is `Err(e)`, the provider is Ollama, and `ollama_rejected_reasoning_effort(&e)` holds, remove `reasoning_effort` from a clone of the body and run `send_with_retry` once more.
- `ollama_rejected_reasoning_effort` is true for `LlmError::Http { status: 400 | 422, body }` when `body`, lowercased, contains `reasoning_effort` or `think`.
  - This is deliberately broader and simpler than upstream's JSON-field parser.
  - It also catches "model does not support thinking" style messages.
  - A false positive costs one extra request that fails the same way.
  - The compatibility re-send is not repeated, and the retry policy is unchanged.
- The debug log stays one file per `generate_summary` call, recording the final outcome, as change 10 established. The logged request is the original body, and the outcome is whichever send finished last.

Alternatives:
- **Use Ollama's native `/api/chat` with `think: false`.** That is a second request/response shape for one provider. Rejected.
- **Remember per endpoint that the field was rejected.** It saves one fast 400 per stage, but needs process state keyed by endpoint and model. Not worth it.

### D6: One stage cleaner; strictness lives at the stage, not in `clean_llm_markdown_output`

Two regexes replace `THINKING_TAG_REGEX` (`processor.rs:11-12`):
- **Envelope:** `(?is)<think(?:ing)?(?:\s[^>]*)?>.*?</think(?:ing)?\s*>`
- **Marker:** `(?i)</?think(?:ing)?(?:\s[^>]*)?>`

`<thinker>` matches neither.

`clean_llm_markdown_output(&str) -> String` keeps its signature and its public re-export, and switches to the envelope regex. A new private `clean_stage_output(stage, raw) -> Result<StageOutput { markdown, reasoning_stripped }, String>` calls it, then:
- returns `"{stage} contained an unterminated reasoning marker"` if the marker regex still matches;
- returns `"{stage} returned no visible summary content after reasoning removal"` if the result is empty.

Every stage uses it: chunk (`"Summary chunk"`), combine (`"Combined summary"`), final (`"Final summary"`), and `run_markdown_transform` (`"Translation pass"` / `"English normalization pass"`). For normalization, the existing `english_markdown_after_normalization_result` already turns a non-cancel `Err` into the pass-1 fallback. For translation, the existing `"Translation to {name} failed: …"` hard failure applies.

Alternative: make `clean_llm_markdown_output` itself return `Result`. That changes a public re-exported function and every call site's error flow for no gain. Rejected.

### D7: Chunk loop: two attempts per chunk, then fail the run

`const MAX_CHUNK_ATTEMPTS: usize = 2`. For each chunk, each attempt does `generate_summary` and then `clean_stage_output("Summary chunk", …)`.
- **Success:** push the cleaned markdown, so the combine prompt only ever sees cleaned text.
- **Error with the cancellation token cancelled:** return `"Summary generation was cancelled"`. It is never retried, and the run ends cancelled, not failed.
- **Error on attempt 1:** `warn!` and try again.
- **Error on attempt 2:** return `"Summary generation could not complete because transcript section {i} of {n} failed after 2 attempts: {e}. Please retry."`

The `chunk_summaries.is_empty()` branch (`448-453`) becomes unreachable and is removed. `successful_chunk_count` then always equals the chunk count.

The run-level failure flows through the existing `update_process_failed` path, which restores `result_backup`, and through the frontend's existing failed-status handling (toast, plus the restored-summary path for regenerations).

Cancellation is detected with `token.is_cancelled()`, not the `e.contains("cancelled")` string match. The string match is kept everywhere else in the processor (surgical).

### D8: Interrupted runs are failed once at startup

A new `SummaryProcessesRepository::fail_interrupted_runs(pool) -> Result<u64, sqlx::Error>` runs the failed-update `SET` clause (status `failed`, `error = 'Summary generation was interrupted because the app closed. Generate the summary again.'`, `result = COALESCE(result_backup, result)`, clear the backup) `WHERE LOWER(status) = 'pending'`.

It is called once from `initialize_database_on_startup`'s normal branch (`database/setup.rs:29-34`), after the pool exists and before `app.manage`. At that point no run can be active in this process, so the update cannot race a live run. The count is logged.

Without this, a run that dies with the app leaves `pending` forever. With frontend resume (D10), that would show a spinner for about 16 minutes and block auto-summary for that meeting permanently.

Alternative: in `api_get_summary`, report `pending` rows that have no registered run as failed. That couples the read path to in-memory state, and it is racy unless registration strictly precedes row creation on every path. Rejected in favor of the race-free one-shot.

### D9: Sidebar polling registry: ref-held, in-flight-guarded, identity-checked

`SidebarProvider` replaces `useState<Map<string, Timeout>>` (`:77`) with `useRef<Map<meetingId, { processId, timer, inFlight }>>`.
- `startSummaryPolling(meetingId, processId, onUpdate)` and `stopSummaryPolling(meetingId, processId?)` become stable `useCallback(…, [])`.
- A stop that names a `processId` acts only when that process is the active entry.
- Each tick:
  - returns at once if its entry is no longer current, or a request is in flight;
  - marks the entry in flight, awaits `getSummary`, and ignores a result whose `start !== processId`;
  - otherwise awaits `onUpdate`;
  - stops on a terminal status;
  - always clears in-flight in `finally`, but only on its own entry.
- An error from either `getSummary` or `onUpdate` reports an error status once and stops polling. A throwing callback can no longer leave a zombie interval.
- The provider-level cleanup runs only on unmount (`useEffect(() => () => …, [])`). The current effect depends on the map and clears every interval whenever any poll starts or stops.
- `activeSummaryPolls` is removed from `SidebarContextType`. It has no consumers, and this change orphans it.
- `MAX_POLLS` and the 5 s interval are unchanged.

### D10: `useSummaryGeneration` hydrates from the stored response and owns its poll lifetime

- **The prop.** New prop `initialSummary?: SummaryStatusResponse | null`. `page.tsx` already calls `getSummary` on load (`:217-317`). It now keeps the raw response in state, guards the fetch with a `cancelled` flag so a slow response for the previous meeting is dropped, and passes the response through `PageContent` (`key={meetingId}`) to the hook.
- **Initial status.** The hook derives it with a lazy `useState` initializer from `initialSummary`, applied only when `initialSummary.meeting_id === meeting.id`:
  - `pending`/`processing` → `regenerating` if the response has summary data, else `processing`;
  - `failed`/`error` → `error`, with the stored error and no toast;
  - anything else → `idle`, which is today's behavior.
  - Because the initializer is synchronous, `page-content.tsx`'s auto-generate effect sees the restored status on its first run.
- **Resume.** A mount effect starts polling for `initialSummary.start` when the restored status is in progress.
- **The callback.** The inline polling callback in `processSummary` (`:168-346`) becomes one `handlePollingResult(result, generationId, isRegeneration)` used by both a fresh start and a resume. It is held in a ref, so ordinary re-renders do not restart polling.
- **Guards.** `mountedRef`, `generationIdRef` (bumped on every start, resume and stop) and `activeProcessIdRef` guard every continuation after an `await`. A result for an unmounted view, another meeting, or an older generation is dropped.
- **Unmount.** The hook stops polling for its active process. It does not cancel the backend run. This replaces `page.tsx`'s cleanup effect (`:189-197`).
- **Stop.** `handleStopGeneration` sends `cancelSummary({ meetingId, processId: activeProcessIdRef.current })`.
- **Late start response.** If `processTranscript` resolves after the generation was superseded by Stop or a new start, the hook cancels that `process_id`. If it resolves after unmount, the hook does nothing: no cancel and no poll. The next visit resumes the run from the stored `pending` state.
- **Analytics.** A resumed run emits no completion analytics, because its start event belongs to the visit that started it. Toasts still fire for outcomes observed while the view is open.
- **Auto-summary.** It is gated twice. `page.tsx`'s `checkAutoGen` additionally requires `!isLoading && summaryResponse?.status === 'idle'`. `page-content.tsx`'s auto-generate effect additionally requires `summaryGeneration.summaryStatus === 'idle'`.

### D11: The IPC change lands with the backend change

`api_cancel_summary` gains a required `process_id: String` (`processId` on the wire). `CancelSummaryArgs` in `lib/ipc/summary.ts` is updated in the same commit as the backend (task group 3), together with the one call site (`useSummaryGeneration.ts:619`) and a minimal `activeProcessIdRef` set from `processTranscript`'s result. That keeps Stop working at every commit. Group 4 then builds resume on that ref.

### D12: Tests

- **Rust.**
  - `llm_client.rs`, using wiremock on `/v1/chat/completions`, covers the compatibility re-send and its bound, an unrelated 400, no field for non-Ollama providers, and null-content and reasoning-field parsing.
  - `processor.rs` covers the cleaner and stage-cleaner unit cases. Chunk semantics are tested end to end through `generate_meeting_summary` against wiremock, with `provider = Ollama`, a small `token_threshold`, and per-request response sequences. The cases are: retry then succeed, fail twice and fail the run, think tags absent from the combine request body (checked with `received_requests()`), and cancel not retried. Failures use 200 responses with empty or reasoning-only content, or a 400 whose body avoids "think", so the transport backoff does not slow the tests.
  - `repositories/summary.rs` uses in-memory SQLite with the real migrations and a seeded `meetings` row (the FK) to cover the CAS matrix, `fail_interrupted_runs`, and the `run_id` round trip.
  - `service.rs` covers registry supersede, stale cleanup, stale cancel, and monotonic start.
- **Frontend.** Adapt upstream's `frontend/tests/hooks/summary-generation.test.tsx`:
  - React rendering uses `react-test-renderer` (a new dev-dependency, 18.3.1, to match React 18) with a fake `setInterval`.
  - Mocks: `@tauri-apps/api/core` `invoke` (the fork's `invokeTyped` calls it) and `@tauri-apps/api/event` (operations-doc gotcha), plus `next/navigation`, `RecordingStateContext`, `sonner`, `@/lib/analytics`, and `@/lib/summary-language-preferences`. The originals are restored in `afterAll`.
  - Upstream's dependencies on `parseSummaryContent`, `readSummaryMetadata` and tracked-attempt analytics are replaced by the fork's `SummaryDataResponse` shape.
  - The test that asserts upstream's analytics model-retention behavior is dropped.

## Risks / Trade-offs

- **[Retry stacking]** One chunk can now cost up to 2 chunk attempts × 3 transport attempts. Against a hung Ollama that is up to 2 × 3 × 300 s before the run fails. → This is accepted as the cost of the user's "retry once" decision. Each HTTP attempt is still bounded, connect errors fail fast, and the manual check (task 5.5) records real timings.
- **[Frontend poll timeout]** `MAX_POLLS = 200` at 5 s (about 16.5 min) can declare a long Ollama run timed out while the backend continues. The run then completes in the DB, and the next visit shows it. Resume restarts the counter, which makes this less likely but does not remove it. → Unchanged here. The manual long-meeting check (5.5) records total duration. If it exceeds about 16 minutes, raising or removing the client timeout is a follow-up. Stale `pending` rows are now handled by D8, so the timeout is no longer the only escape.
- **[Broad compat matcher]** A 400/422 from Ollama that mentions "think" for another reason triggers one useless re-send. → It is bounded to one extra request, and the second error is surfaced.
- **[Stricter cleaning rejects borderline output]** A model that writes a literal `<think>` in prose now fails the stage. → Chunks get one retry, and the error names the cause. This is an explicit spec choice: never save reasoning.
- **[Stored failures now shown on open]** A meeting whose last regeneration failed weeks ago shows that error each time it opens, until the next run. → This is the honest state and matches upstream. Users dismiss it by regenerating.
- **[Lost completion analytics]** A run started in one visit and observed in another emits a start event but no completion event (D10). → Accepted. The alternative attributes it to whatever model is configured now.
- **[Sibling ordering]** `port-upstream-041-quick-fixes` edits `llm_client.rs` (Claude parsing) and `processor.rs` (`chunk_text`). → Task 0.1 blocks this change on its archive. Groups 1-2 then rebase onto its code. This change does not touch `chunk_text` or Claude block selection.
- **[Legacy import path]** `import_and_initialize_database` does not run D8. → A legacy DB with a stale `pending` row is not a realistic case. Deferred (Open Questions).

## Migration Plan

- No schema migration. D8 is a data fix-up that runs at every startup, is idempotent, and only touches rows already stuck in `pending`.
- **Deploy.** Task groups 1→4 land in order, each green on its own: 1 is client and cleaning, 2 is the chunk loop, 3 is run scoping plus the IPC cancel arg, 4 is the frontend.
- **Rollback.** Revert the commits. Rows marked failed by D8 stay failed, and the restored `result` is kept. The run-id format is backward-readable, because older code ignores `start` and never parsed `process_id`.

## Open Questions

- Should `fail_interrupted_runs` also run after `import_and_initialize_database` and `initialize_fresh_database`? A fresh DB has no rows, and a legacy import with a stale `pending` row is unlikely. Adding the call later is one line and does not affect the specs.
