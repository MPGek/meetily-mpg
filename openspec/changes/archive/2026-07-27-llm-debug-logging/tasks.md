## 1. New Module: `summary/debug_log.rs`

- [x] 1.1 Create `summary/debug_log.rs` with `pub const DEBUG: bool = true` and `LLM_ITERATION_COUNTER: AtomicU64`
- [x] 1.2 Implement `fn reset_iteration_counter()` that sets the counter to 0
- [x] 1.3 Implement `fn next_iteration() -> u64` that atomically increments and returns the previous value
- [x] 1.4 Implement `fn debug_log_path(folder: &Path, start_time: &DateTime<Local>, iteration: u64) -> PathBuf` returning `{folder}/yyyymmdd_hhmmss_it_{N}.log`
- [x] 1.5 Implement `fn write_debug_log(log_dir: &Path, request: &DebugLogEntry, response: &DebugLogResult)` that writes the JSON log file
- [x] 1.6 Define `DebugLogEntry` struct for request data (timestamp, provider, model, request_json, extra params)
- [x] 1.7 Define `DebugLogResult` enum with `Success { content, status_code, elapsed }` and `Error { message, elapsed }` variants
- [x] 1.8 Export `DEBUG`, `reset_iteration_counter`, `next_iteration`, `write_debug_log`, `DebugLogEntry`, `DebugLogResult` from the module

## 2. Wire Module into `summary/mod.rs`

- [x] 2.1 Add `pub(crate) mod debug_log;` to `summary/mod.rs`
- [x] 2.2 Re-export public items: `DEBUG`, `reset_iteration_counter`, `next_iteration`, `write_debug_log`

## 3. Update `summary/llm_client.rs`

- [x] 3.1 Add `debug_log_dir: Option<PathBuf>` parameter to `generate_summary()` (before `cancellation_token`)
- [x] 3.2 At function start (after cancellation check), call `next_iteration()` and build `DebugLogEntry` with the request payload, provider, model
- [x] 3.3 After the response is received (both success and error paths), call `write_debug_log()` with the entry and result
- [x] 3.4 Ensure the log captures HTTP status code, elapsed time, and full request/response bodies
- [x] 3.5 Handle the BuiltInAI early-return path separately (the debug log entry should still be written)

## 4. Update `summary/processor.rs`

- [x] 4.1 Add `debug_log_dir: Option<PathBuf>` parameter to `generate_meeting_summary()` (after `cancellation_token`, before `summary_language`)
- [x] 4.2 Pass `debug_log_dir` to every `generate_summary()` call in the function (chunk loop, combine, final report, translate, normalize)

## 5. Update `summary/service.rs`

- [x] 5.1 In `process_transcript_background()`, resolve meeting folder via `resolve_meeting_folder()` (import from `commands.rs` or call the DB query directly)
- [x] 5.2 Call `reset_iteration_counter()` before starting summary generation
- [x] 5.3 Pass `Some(meeting_folder)` or `None` as `debug_log_dir` to `generate_meeting_summary()`

## 6. Update BuiltInAI Sidecar `summary_engine/client.rs`

- [x] 6.1 Add `debug_log_dir: Option<PathBuf>` parameter to `generate_with_builtin()`
- [x] 6.2 At function start, call `next_iteration()` and build `DebugLogEntry`
- [x] 6.3 After the sidecar responds (success or error), call `write_debug_log()`
- [x] 6.4 Update callers of `generate_with_builtin()` to pass the `debug_log_dir`

## 7. Verify and Test

- [x] 7.1 Run `cargo build --package meetily` and fix any compilation errors
- [ ] 7.2 Verify log files appear in the meeting folder when `DEBUG = true` (manual — requires running app with a recorded meeting)
- [ ] 7.3 Verify log files contain valid JSON with request/response data (manual — inspect output from 7.2)
- [ ] 7.4 Verify iteration counter resets between sessions (manual — run two summaries for same meeting)
- [ ] 7.5 Set `DEBUG = false` and verify no log files are written (manual — change const, rebuild, run)
