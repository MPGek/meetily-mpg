# Tasks

## 0. Preconditions

- [x] 0.1 Confirm `port-upstream-041-quick-fixes` is archived (applied): #603 `chunk_text` coverage and #694 Claude thinking-block parsing are in `summary/processor.rs` / `summary/llm_client.rs`. Re-verify every file:line cited in proposal.md and design.md against that code, and record any drift as a note under this task. Verify: `openspec list` shows it archived, and `git log --oneline -- frontend/src-tauri/src/summary/llm_client.rs` shows its commit.
  - Note (2026-10-02): archived as `archive/2026-10-02-port-upstream-041-quick-fixes` (not in `openspec list`); `llm_client.rs` log shows 4210193 (#694), `processor.rs` has c0cd183 (#603). Drift, all from those two commits: `llm_client.rs` `generate_summary` is now 129-409 (was 114-394), the cancel `select!` 322-347 (305-330), `.json()` 354/368 (336/352), request body 245-282 (230-265); `ChatRequest` 17-27 and `MessageContent` 40-43 unchanged. `processor.rs` shifted +5: `generate_meeting_summary` 332-609, chunk loop 405-451 (push 440, skip 443-449), empty-chunks branch 453-458, combine 467-494, final report 526-542, completion `info!` 607, `run_markdown_transform` 612-655, `clean_llm_markdown_output` 269-289; regex 11-12 unchanged. `service.rs`, `commands.rs`, `repositories/summary.rs`, `setup.rs` and the frontend citations match.
- [x] 0.2 Record baselines in this task's note. Run each command from the folder named in docs/CODEBASE_MAP_OPERATIONS.md:
  - `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands` (passed/failed/ignored counts);
  - `cargo test -p meetily --lib summary` and `cargo test -p meetily --lib database::repositories::summary` (counts);
  - `cargo clippy -p meetily --all-targets --message-format=short 2>&1 | grep -c "^warning"` (warning count);
  - `bun test tests/` and `pnpm exec tsc --noEmit -p .` in `frontend/` (pass counts, tsc clean or not).

  Verify: all five numbers are written down.
  - Note (2026-10-02): full lib suite 563 passed / 0 failed / 9 ignored; `--lib summary` 97 passed; `--lib database::repositories::summary` 0 tests; clippy `grep -c "^warning"` = 18 (mostly build-script noise lines), the real per-location count `grep -c ": warning"` = 32 (pre-existing in touched files: `llm_client.rs:94` `from_str` should-implement-trait, `templates/defaults.rs:15`); `bun test tests/` 99 pass / 0 fail; `tsc --noEmit` clean. The ": warning" count is the one compared in later tasks.

## 1. LLM client compatibility and stage cleaning (proposal a + c)

- [x] 1.1 In `summary/llm_client.rs`:
  - add `reasoning_effort: Option<&'static str>` (skip if none) to `ChatRequest`, set to `Some("none")` only for `LLMProvider::Ollama`;
  - make `MessageContent` `{ content: Option<String>, reasoning: Option<String>, reasoning_content: Option<String> }` with `#[serde(default)]`;
  - read the visible text as `content.unwrap_or_default().trim()`;
  - `info!` the discarded reasoning length when non-empty (design D4/D5).

  Verify: unit tests (a) the Ollama body has `reasoning_effort == "none"` and the OpenAI/Groq/OpenRouter/CustomOpenAI bodies do not; (b) `{"content":null,"reasoning_content":"x"}` and a message with no `content` key both parse to empty text; (c) `content` plus `reasoning` yields only `content`. Run `cargo test -p meetily --lib summary::llm_client`.
  - Note (2026-10-02): the OpenAI-compatible body is built by a new private `build_chat_request` helper (moved out of `generate_summary` unchanged apart from `reasoning_effort`) so test (a) can cover OpenAI/Groq/OpenRouter, whose URLs are hardcoded and cannot be pointed at wiremock. Visible text / discarded-reasoning length are `MessageContent::visible_text` / `discarded_reasoning_len`.
- [x] 1.2 Add `ollama_rejected_reasoning_effort(&LlmError) -> bool`: true for `LlmError::Http { status: 400 | 422, body }` when the lowercased body contains `reasoning_effort` or `think`. In `generate_summary`, wrap the send in one async block inside the existing cancellation `select!`. On a matching Ollama error, re-send once with `reasoning_effort` removed from a cloned body (design D5).

  Verify: unit test of the matcher (400/422 positive; 401, 500, and a 400 `"invalid model"` negative). Wiremock tests on `/v1/chat/completions`:
  - first request 400 `{"error":{"param":"reasoning_effort"}}`, second 200 → success, 2 requests received, the second body has no `reasoning_effort`;
  - both 400 with the reasoning body → error surfaced, exactly 2 requests;
  - 400 `{"error":"invalid model"}` → error, exactly 1 request;
  - a non-Ollama provider pointed at the mock with a reasoning-400 → exactly 1 request.

  Run `cargo test -p meetily --lib summary::llm_client`.
  - Note (2026-10-02): the 401 negative case is `LlmError::AuthFailed { status: 401 }`, since `send_with_retry` never returns 401 as `LlmError::Http`; the non-Ollama wiremock case uses `CustomOpenAI` pointed at the mock.
- [x] 1.3 In `summary/processor.rs`:
  - replace `THINKING_TAG_REGEX` with the envelope regex `(?is)<think(?:ing)?(?:\s[^>]*)?>.*?</think(?:ing)?\s*>` and the marker regex `(?i)</?think(?:ing)?(?:\s[^>]*)?>`;
  - keep `clean_llm_markdown_output`'s signature and re-export, using the envelope regex;
  - add private `clean_stage_output(stage, raw) -> Result<StageOutput { markdown, reasoning_stripped }, String>` with the two error messages from design D6.

  Verify: unit tests for closed envelopes anywhere (mixed case, attributes, inside a code fence), `<thinker>` kept unchanged, the unclosed `<think>…`, stray `</thinking>` and attributed-unclosed cases → marker error, reasoning-only and empty-fence output → empty error. Run `cargo test -p meetily --lib summary::processor`.
- [x] 1.4 Apply `clean_stage_output` to the final-report stage (`"Final summary"`), the combine stage (`"Combined summary"`), and `run_markdown_transform` (stage label = its `failure_label`). OR `reasoning_stripped` across stages in `generate_meeting_summary` and include it in the completion `info!` line. Do not change the return type (design D4).

  Verify: unit tests — empty normalization output still falls back to the pass-1 markdown (`english_markdown_after_normalization_result`); a cancelled normalization still errors. Run `cargo test -p meetily --lib summary` (all pass) and `cargo check -p meetily`.
  - Note (2026-10-02): the cancelled-normalization case is covered by the existing `cancelled_english_normalization_is_not_swallowed`; the new test is `empty_normalization_output_falls_back_to_pass_one_markdown`. `run_markdown_transform`, `translate_markdown` and `normalize_markdown_to_english` now return the private `StageOutput` so the flag can be ORed. `cargo test -p meetily --lib summary`: 111 passed (97 + 14 new).
- [x] 1.5 Commit group 1 on its own. Verify: `cargo test -p meetily --lib summary` passes and `cargo clippy -p meetily --all-targets --message-format=short` shows no new warnings against the 0.2 baseline.
  - Note (2026-10-02): clippy 32 `: warning` lines, same as baseline (only pre-existing `llm_client.rs` `from_str`, now line 170). rustfmt applied to the two touched files; it changed only the new code.

## 2. Chunk failure semantics (proposal b)

- [ ] 2.1 Rewrite the chunk loop in `generate_meeting_summary` (`processor.rs` ~400-459) per design D7:
  - `MAX_CHUNK_ATTEMPTS = 2`;
  - each attempt is `generate_summary` + `clean_stage_output("Summary chunk", …)`;
  - push only cleaned markdown;
  - a cancelled token → return the cancelled error with no retry;
  - a second failure → return `"Summary generation could not complete because transcript section {i} of {n} failed after 2 attempts: {e}. Please retry."`;
  - remove the now-unreachable empty-chunks branch.

  Verify: `cargo check -p meetily`.
- [ ] 2.2 Add end-to-end tests that call `generate_meeting_summary` with `LLMProvider::Ollama`, an `ollama_endpoint` pointing at a wiremock server, and a `token_threshold` small enough to produce 3 chunks. Cover:
  - (a) chunk 2's first response is 200 with empty content and its second is valid → `Ok`, `successful_chunk_count == 3`;
  - (b) chunk 2 returns empty content twice → `Err` containing `"transcript section 2 of 3"`, and no request with the combine prompt (`<summaries>`) or final-report prompt (`<transcript_chunks>`) was received;
  - (c) chunk outputs wrapped in `<think>…</think>` → the combine request body (from `received_requests()`) contains none of the think text;
  - (d) the token is cancelled after chunk 1 → `Err` "cancelled" and no retry request for chunk 2.

  Avoid 5xx/timeouts so transport backoff does not slow the tests. Run `cargo test -p meetily --lib summary::processor`.
- [ ] 2.3 Commit group 2 on its own. Verify: `cargo test -p meetily --lib summary` passes and clippy shows no new warnings against the 0.2 baseline.

## 3. Run-scoped DB writes, cancellation and IPC (proposal d)

- [ ] 3.1 In `database/repositories/summary.rs`:
  - `create_or_reset_process(pool, meeting_id, started_at)` binds the caller's `started_at` for `start_time` (it no longer reads `now` internally);
  - `update_process_completed/failed/cancelled` take `started_at`, add `AND start_time = ? AND LOWER(status) = 'pending'`, and return `Result<bool, _>`;
  - add `fail_interrupted_runs(pool) -> Result<u64, _>` (design D3/D8).

  Verify: tests on in-memory SQLite with the real migrations and a seeded `meetings` row:
  - completed-then-cancelled keeps `completed`;
  - cancelled-then-completed keeps `cancelled` with the restored previous result;
  - each of the three updates with a stale `started_at` returns `false` and leaves the row `PENDING`;
  - `fail_interrupted_runs` turns a `PENDING` row into `failed` with the interrupted error and the restored backup, and leaves `completed` rows alone;
  - `start_time` read back equals the bound value.

  Run `cargo test -p meetily --lib database::repositories::summary`.
- [ ] 3.2 In `summary/service.rs`, per design D1/D2:
  - replace the registry with `HashMap<meeting_id, { started_at, token }>`;
  - add `register_run(meeting_id) -> (DateTime<Utc>, CancellationToken)`. It is monotonic per meeting and cancels the previous token;
  - `cancel_summary(meeting_id, started_at) -> bool` and `cleanup_run(meeting_id, started_at)` act only on a matching entry;
  - add `run_id(&DateTime<Utc>) -> String` (RFC 3339, nanoseconds, `Z`);
  - `process_transcript_background` takes `started_at` and `token` instead of registering. Every failed/completed/cancelled write passes `started_at` and logs `Ok(false)` as a skipped stale write;
  - the meeting rename runs only after the completed write returns `Ok(true)`;
  - the existing cache and language-detection code is kept unchanged.

  Verify: unit tests — registering run B cancels run A's token; `cleanup_run(A)` after B registered keeps B cancellable; `cancel_summary(meeting, A)` returns `false` and leaves B's token uncancelled; two `register_run` calls in a row yield strictly increasing starts; `run_id` round-trips through `DateTime::parse_from_rfc3339`. Run `cargo test -p meetily --lib summary::service`.
- [ ] 3.3 In `summary/commands.rs`, per design D2/D3:
  - `api_process_transcript` calls `register_run` first. A `create_or_reset_process` error removes the registration. A `save_transcript_data` error does a CAS-fail of the run with the message, removes the registration, and returns `Err`;
  - it passes `started_at` and the token into the spawn, and returns `process_id = run_id(started_at)`;
  - `api_get_summary`'s `start` uses `run_id`;
  - `api_cancel_summary` takes a required `process_id: String`, parses it (invalid → `Err("Invalid summary process ID")`), and cancels and CAS-writes only that run, with messages for cancelled, already finished, and no active run.

  Verify: `cargo check -p meetily`, and `grep -n "cancel_summary\|create_or_reset_process\|update_process_" frontend/src-tauri/src` shows no call without `started_at`.
- [ ] 3.4 In `database/setup.rs`'s normal startup branch, call `SummaryProcessesRepository::fail_interrupted_runs` once after the pool is created and before `app.manage`, and log the count. A failure is logged and does not block startup. Verify: `cargo check -p meetily`, plus the 3.1 test covering the query.
- [ ] 3.5 Keep Stop working at this commit (design D11):
  - `frontend/src/lib/ipc/summary.ts` gets `CancelSummaryArgs { meetingId; processId }` for `cancelSummary`;
  - `useSummaryGeneration.ts` stores `result.process_id` in an `activeProcessIdRef` and passes it to `cancelSummary`. No other frontend behavior changes.

  Verify: `pnpm exec tsc --noEmit -p .` and `bun test tests/` in `frontend/` pass (baseline counts).
- [ ] 3.6 Commit group 3 on its own. Verify: `cargo test -p meetily --lib summary` and `cargo test -p meetily --lib database` pass, clippy shows no new warnings against the 0.2 baseline, and tsc is clean.

## 4. Frontend progress resume and stale-result guards (proposal e)

- [ ] 4.1 Add dev-dependencies `react-test-renderer@18.3.1` and `@types/react-test-renderer@18.3.1` with `pnpm add -D` in `frontend/`. Verify: `pnpm-lock.yaml` updated, and `bun test tests/` and tsc are still green.
- [ ] 4.2 Rewrite summary polling in `components/Sidebar/SidebarProvider.tsx` per design D9:
  - a ref-held `Map<meetingId, { processId, timer, inFlight }>`;
  - stable `startSummaryPolling` / `stopSummaryPolling(meetingId, processId?)`;
  - an in-flight guard, and `result.start !== processId` results ignored;
  - `onUpdate` is awaited;
  - read and callback errors report once and stop;
  - unmount-only cleanup;
  - `activeSummaryPolls` removed from the context type and value (no consumers — re-check with `grep -rn activeSummaryPolls frontend/src`).

  Verify: tests in `frontend/tests/hooks/summary-generation.test.tsx` — a throwing callback and a failing read each stop the poll (no timers left); one meeting's poll ending does not clear another's; a result with another `start` is ignored.
- [ ] 4.3 In `hooks/meeting-details/useSummaryGeneration.ts`, per design D10:
  - add an `initialSummary` prop, with a lazy `useState` status initializer used only when `meeting_id` matches (pending → processing/regenerating, failed/error → error with the stored message and no toast, else idle);
  - add a mount effect that resumes polling for `initialSummary.start`;
  - extract a ref-held `handlePollingResult` shared by start and resume;
  - add `mountedRef`/`generationIdRef` guards after every `await`;
  - on unmount, stop polling the active process but do not cancel it;
  - a start response that arrives after a superseding Stop or start cancels that `process_id`, and one that arrives after unmount does nothing;
  - a resumed run emits no completion analytics.

  Verify: the hook tests in 4.5.
- [ ] 4.4 Wire the pages:
  - `app/meeting-details/page.tsx` keeps the `getSummary` response in state, guards the fetch effect with a `cancelled` flag, passes `initialSummary` and `key={meetingId}` to `PageContent`, gates `checkAutoGen` on `!isLoading && summaryResponse?.status === 'idle'`, and removes its stop-polling cleanup effect (now in the hook);
  - `app/meeting-details/page-content.tsx` forwards `initialSummary` to the hook and adds `summaryGeneration.summaryStatus === 'idle'` to the auto-generate condition.

  Verify: `pnpm exec tsc --noEmit -p .`, and a test in 4.5 that a pending stored status with `shouldAutoGenerate` starts no `api_process_transcript`.
- [ ] 4.5 Adapt upstream `frontend/tests/hooks/summary-generation.test.tsx` (e4cc94b) to the fork, per design D12:
  - mock `@tauri-apps/api/core` `invoke` and `@tauri-apps/api/event`, plus `next/navigation`, `RecordingStateContext`, `sonner`, `@/lib/analytics`, `@/lib/summary-language-preferences`, restoring the originals in `afterAll`;
  - use a fake `setInterval`;
  - use the fork's `SummaryStatusResponse` and `SummaryDataResponse`;
  - drop the analytics model-retention case.

  Cases, one per `summary-progress-tracking` scenario:
  - resume after leave and return, then completion with the new title;
  - regeneration resume keeps the old notes;
  - Stop after return sends `{ meetingId, processId: <start> }`;
  - an ordinary re-render keeps the same timer;
  - failed-while-away shows the stored error with no toast;
  - completed-while-away shows no toast or analytics;
  - a pending status blocks auto-generation;
  - a status for another meeting stays idle;
  - a late completion from meeting A does not change B;
  - an old in-flight poll does not stop the resumed poll;
  - leaving before the start response arrives sends no cancel, and resume works later.

  Verify: `bun test tests/` passes, the new file included.
- [ ] 4.6 Commit group 4 on its own. Verify: `bun test tests/` passes, `pnpm exec tsc --noEmit -p .` is clean, and `pnpm exec next lint` shows no new errors in the touched files.

## 5. Integration verification

- [ ] 5.1 Run `cargo test -p meetily --lib -- --skip audio::playback_monitor --skip audio::system_audio_commands` from the repo root. Verify: 0 failed, and passed = baseline (0.2) + the new tests.
- [ ] 5.2 Run `cargo clippy -p meetily --all-targets --message-format=short`. Verify: the warning count is ≤ the 0.2 baseline, and no warning points into a file this change touched.
- [ ] 5.3 Run `bun test tests/` and `pnpm exec tsc --noEmit -p .` in `frontend/`. Verify: all pass, and tsc is clean.
- [ ] 5.4 Run `openspec validate summary-run-integrity --strict`. Verify: valid.
- [ ] 5.5 Manual check in the dev app (`frontend/dev-gpu.bat`) with Ollama and a reasoning model (e.g. `qwen3`) on a long meeting that produces several chunks. Record each item below with timings in this task's note, including total run duration against the ~16.5 min client poll timeout (design Risks):
  - the summary contains no reasoning text, and the per-call debug logs show either `reasoning_effort` accepted or exactly one compatibility re-send;
  - leaving for another meeting mid-run and returning shows generation in progress, then the completed summary;
  - Stop after returning cancels the run (status `cancelled`, previous summary restored);
  - quitting the app mid-run and restarting shows the meeting's summary as failed with the interrupted message, and a new run can be started.
- [ ] 5.6 Run `graphify update .` from the repo root. Verify: the command succeeds.
