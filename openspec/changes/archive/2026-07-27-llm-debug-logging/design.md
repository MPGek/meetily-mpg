## Context

The summary pipeline (`service.rs` → `processor.rs` → `llm_client.rs`) makes LLM calls to 7 providers (OpenAI, Claude, Groq, Ollama, OpenRouter, BuiltInAI, CustomOpenAI) plus `summary_engine/client.rs` for the BuiltInAI local sidecar path. Currently only a single `info!("🐞 LLM Request...")` / `info!("🐞 LLM Response...")` log line exists per call — no payload capture. Audio and transcript files are stored in per-meeting folders like `{base_path}/{MeetingName}_{YYYY-MM-DD_HH-MM}/`.

The meeting folder path is persisted in `meetings.folder_path` in SQLite and resolvable via `resolve_meeting_folder()` in `commands.rs`. Logging is hardcoded to `info` level in `lib.rs:391` and `main.rs:10`.

## Goals / Non-Goals

**Goals:**
- Add a `DEBUG` variable (default `true`) that enables per-LLM-call logging to the meeting folder
- Log files named `yyyymmdd_hhmmss_it_N.log` where N is the sequential iteration number of each LLM call within a summary session
- Each log file captures: timestamp, provider, model, full request payload (system prompt, user prompt/messages), full response payload, elapsed time, and error details
- BuiltInAI sidecar calls (`summary_engine/client.rs`) are also covered

**Non-Goals:**
- Not a general-purpose logging framework — only LLM interaction payloads
- No UI for enabling/disabling debug (environment variable or code constant is sufficient)
- No log rotation, compression, or retention policy (user manages folder)

## Decisions

1. **`DEBUG` as a `const bool` in a new `summary/debug_log.rs` module** rather than an env var. Rationale: simpler, compile-time, matches project style (no existing env var pattern for debug flags). Default `true` per requirement. Users who want to suppress logs change the constant.

2. **New module `summary/debug_log.rs`** containing the iteration counter (thread-safe `AtomicU64`) and `write_debug_log()` function. Keeps the concern isolated — `llm_client.rs` and `processor.rs` don't need to know about file I/O logic.

3. **Iteration counter shared via atomic static** — no plumbing through the deep call chain. `generate_summary()` and `generate_with_builtin()` increment the counter on each call. The counter resets when a new summary session starts (`process_transcript_background()` re-initializes it).

4. **Single file per call** (one log per LLM interaction) rather than appending to a single file. Rationale: easier to inspect individual calls, no file locking concerns across concurrent LLM calls, matches the user's stated format.

5. **Meeting folder path passed through the call chain** — `service.rs` resolves it via existing `resolve_meeting_folder()`, passes to `processor.rs`'s `generate_meeting_summary()`, which passes to `llm_client.rs`'s `generate_summary()`. All as `Option<PathBuf>`. If the folder is unavailable (no DB entry), logs are silently skipped.

6. **BuiltInAI coverage** — `summary_engine/client.rs`'s `generate_with_builtin()` receives the same `Option<PathBuf>` parameter and writes debug logs for its local sidecar calls.

7. **Error logging** — On success, the full response body is logged. On error, the error message and partial response (if available) are logged. The error is still propagated to the caller.

## Risks / Trade-offs

- **Disk space**: Each LLM response can be large (especially full summaries). Risk mitigated by `DEBUG = true` being opt-out and files being user-managed in the meeting folder. Low risk since summaries are already stored as DB blobs.
- **Sensitive data**: LLM request bodies may contain API keys in headers (already), but the *payload* (messages/prompts) is user transcript content — same data already stored in transcripts. API keys are in HTTP headers, not in the request body we log. Low risk.
- **Concurrent LLM calls**: The iteration counter is atomic, but two overlapping calls might produce `it_1` and `it_2` simultaneously. Fine — each file is independent. No shared file handle.
- **File name timestamp granularity**: If two LLM calls happen within the same second, they'd collide. Mitigation: `it_N` suffix differentiates them; if somehow the same N, the second write overwrites. Acceptable for single-threaded-per-meeting usage.
