## Why

LLM calls to remote and local providers are a black box — when summaries fail, produce incorrect output, or behave unexpectedly, there is no record of what was sent or received. Debugging requires re-running with instrumentation, which is often impossible for transient issues or for users who cannot reproduce the exact transcript. A toggleable debug log that captures every LLM interaction alongside the audio and transcript files would enable post-mortem analysis without re-running.

## What Changes

- Add a `DEBUG` compile-time/runtime variable defaulting to `true` (opt-out) that controls LLM interaction logging
- When `DEBUG` is true, every LLM request and response in the summary pipeline is written to a log file in the same meeting folder where audio and transcripts are stored
- Log files follow the naming pattern `yyyymmdd_hhmmss_it_N.log` where `N` is the sequential iteration number of each LLM call within that meeting's summary session
- Each log file captures: timestamp, provider name, model name, full request payload (messages/prompt), full response payload, response timing, and error details if applicable
- No new dependencies — use existing `serde_json` and `std::fs` for file I/O

## Capabilities

### New Capabilities
- `llm-debug-logging`: Configurable debug logging of all LLM request/response payloads to meeting-local files with sequential iteration tracking

### Modified Capabilities
- None

## Impact

- **`summary/llm_client.rs`**: Core `generate_summary()` function gains debug logging of request and response payloads
- **`summary/processor.rs`**: Iteration counter tracked per meeting summary session; counter passed into LLM call context
- **`summary/service.rs`**: Debug flag initialized from config/environment; meeting folder path passed through to processor
- **`summary/commands.rs`**: `resolve_meeting_folder()` already exists and provides the target directory
- No new external dependencies — existing `serde_json`, `log`/`tracing`, and `std::fs` are sufficient
