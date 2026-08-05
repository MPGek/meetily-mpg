# llm-debug-logging Specification

## Purpose
TBD - created by archiving change llm-debug-logging. Update Purpose after archive.
## Requirements
### Requirement: DEBUG flag controls logging
The system SHALL expose a `DEBUG` constant boolean, defaulting to `true`, that controls whether per-call LLM interaction logs are written. When `DEBUG` is `true`, every LLM API call SHALL produce a log file. When `DEBUG` is `false`, no log files SHALL be written.

#### Scenario: DEBUG enabled writes logs
- **WHEN** `DEBUG` is `true` and an LLM call completes
- **THEN** a log file SHALL be written to the meeting folder

#### Scenario: DEBUG disabled skips logs
- **WHEN** `DEBUG` is `false` and an LLM call completes
- **THEN** no log file SHALL be written

### Requirement: Log file naming
The system SHALL write each LLM interaction log with the filename pattern `yyyymmdd_hhmmss_it_N.log` where:
- `yyyymmdd_hhmmss` is the local timestamp when the LLM call started
- `N` is a sequential non-negative integer iteration counter, starting from 0 for the first LLM call in each summary session

#### Scenario: First call uses iteration 0
- **WHEN** the first LLM call of a summary session starts at 2026-07-24 16:30:45
- **THEN** the log file SHALL be named `20260724_163045_it_0.log`

#### Scenario: Second call increments iteration
- **WHEN** a second LLM call starts in the same session
- **THEN** the log file SHALL use `it_1` in its filename

### Requirement: Log file location
The system SHALL write log files to the meeting folder (the same directory that contains `audio.mp4`, `transcripts.json`, and `metadata.json`). The meeting folder SHALL be resolved from the `meetings.folder_path` database column at the start of each summary session.

#### Scenario: Available meeting folder
- **WHEN** the meeting has a `folder_path` in the database and the directory exists
- **THEN** log files SHALL be written into that directory

#### Scenario: Missing meeting folder
- **WHEN** the meeting has no `folder_path` or the directory does not exist
- **THEN** log files SHALL be silently skipped (no error, no crash)

### Requirement: Log file content — request payload
Each log file SHALL contain a JSON-serialized representation of the LLM request, including:
- Timestamp of the call start (ISO 8601)
- Provider name (e.g., `"OpenAI"`, `"Claude"`, `"Ollama"`)
- Model name
- System prompt (full text)
- User prompt / messages array (full text)
- Any provider-specific parameters (max_tokens, temperature, top_p)

#### Scenario: OpenAI-compatible request
- **WHEN** a request is sent to an OpenAI-compatible provider
- **THEN** the log SHALL contain the full `ChatRequest` JSON body including model, messages, and optional parameters

#### Scenario: Claude-specific request
- **WHEN** a request is sent to Anthropic Claude
- **THEN** the log SHALL contain the full `ClaudeRequest` JSON body including system, model, max_tokens, and messages

#### Scenario: BuiltInAI sidecar request
- **WHEN** a request is sent to the local BuiltInAI sidecar (`generate_with_builtin`)
- **THEN** the log SHALL contain the system prompt, user prompt, and model name

### Requirement: Log file content — response payload
Each log file SHALL contain a JSON-serialized representation of the LLM response, including:
- Timestamp of the call end (ISO 8601)
- Elapsed time in seconds
- HTTP status code (for HTTP-based providers)
- Full response body (the parsed content from the LLM)
- On error: the error message and any available partial response body

#### Scenario: Successful response
- **WHEN** an LLM call succeeds and returns a response
- **THEN** the log SHALL contain the response content, status, and elapsed time

#### Scenario: Failed response
- **WHEN** an LLM call fails (network error, timeout, API error)
- **THEN** the log SHALL contain the error message and elapsed time

### Requirement: Iteration counter scoping
The iteration counter SHALL be scoped to a single summary session (one `process_transcript_background` invocation). The counter SHALL be reset to 0 at the start of each session.

#### Scenario: Sequential sessions reset counter
- **WHEN** a first summary session completes and a second summary session starts
- **THEN** the counter SHALL restart at 0 for the second session

### Requirement: All LLM call sites covered
Every function that makes an LLM API call SHALL write debug logs when `DEBUG` is enabled:
- `generate_summary()` in `llm_client.rs` for all HTTP-based providers
- `generate_with_builtin()` in `summary_engine/client.rs` for local sidecar

#### Scenario: HTTP provider LLM call is logged
- **WHEN** `generate_summary()` is called with any HTTP provider
- **THEN** a debug log file SHALL be written

#### Scenario: BuiltInAI sidecar call is logged
- **WHEN** `generate_with_builtin()` is called
- **THEN** a debug log file SHALL be written

