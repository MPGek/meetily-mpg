# Design

## Context

See `proposal.md` for motivation. Current state that shapes the approach:

- `frontend/src-tauri/src/summary/llm_client.rs::generate_summary()` (lines 115-330) is the single dispatch point for every actual LLM chat call (chunk summarization, chunk-combine, final report — called from `summary/processor.rs:415,469,520,629`). It already receives a `&Client` from its caller rather than constructing one itself; that client is created fresh per summary session at `summary/service.rs:535` (`let client = reqwest::Client::new();`), so pooling exists only within one session, never across sessions or with the five providers' own model-listing calls.
- `generate_summary` has a single `REQUEST_TIMEOUT_DURATION: Duration = Duration::from_secs(300)` (llm_client.rs:9) applied via `.timeout(REQUEST_TIMEOUT_DURATION)` on the request builder, but on timeout it returns the string `"LLM request timed out after 60 seconds"` (llm_client.rs:280, 301) — a stale message from an earlier 60s timeout that no longer matches the real 300s value. It has no retry of any kind: a single 503 or a single dropped connection fails the whole call (and, for multi-chunk summaries, that specific chunk).
- The five provider modules' model-listing commands each build their own client: `ollama/ollama.rs:162` (`get_models_via_http_async`, called from a 2-retry bespoke loop `get_models_via_http_with_retry` at lines 126-157 with its own `MAX_RETRIES`/`INITIAL_BACKOFF_MS` constants), `openai/openai.rs:130` (`get_openai_models`), `anthropic/anthropic.rs:100` (`get_anthropic_models`), `groq/groq.rs:98` (`get_groq_models`) — these three have no retry and instead fall back to a small hardcoded model list on any failure — and `openrouter/openrouter.rs:43` (`get_openrouter_models`), which is also the only one on `reqwest::blocking::Client` inside an otherwise-non-async `#[command] pub fn` (not `async fn`), so it blocks a Tauri command-handler thread for the duration of the HTTP call.
- Ollama's retry loop treats almost every error as retryable except a string match on `"Invalid endpoint"` or `"404"` (llm_client.rs — no, this is `ollama.rs:141`: `if e.contains("Invalid endpoint") || e.contains("404") { return Err(e); }`), so a 401/403-shaped error string that doesn't happen to contain "404" would currently be retried by Ollama's loop — this is exactly the "retries an auth failure" bug the new shared layer must not repeat.
- The frontend never inspects specific error text to branch behavior; `useSummaryGeneration.ts:196-198` takes whatever string comes back from the backend's `Result<T, String>` (via a polled `pollingResult.error`) and shows it directly in a toast/error state. This means the exact wording of `LlmError`'s `Display` matters for user-visible continuity, but no frontend logic parses it, so wording can be preserved without any frontend change.
- `once_cell = "1.17.1"` is already a dependency (used the same way in `ollama/ollama.rs:14` for `METADATA_CACHE`/`DOWNLOADING_MODELS`), so a `Lazy<Client>` static needs no new dependency. `reqwest = "0.11"` already has the `blocking` feature enabled (only `openrouter.rs` uses it) alongside `json`/`stream`/`multipart`.
- No HTTP mocking crate (`wiremock`, `httpmock`, `mockito`) is currently a dev-dependency of `frontend/src-tauri/Cargo.toml` (only `tempfile`, `infer`, `criterion`, `memory-stats`, `strsim`, `futures`, `tracing-subscriber`).

## Goals / Non-Goals

**Goals:**
- One process-wide `reqwest::Client` shared by every LLM-provider call site (summary generation and model listing), so connections are pooled and configuration (timeouts, user-agent) lives in one place.
- A `send_with_retry` helper with a single, auditable retry policy: bounded attempts, exponential backoff with jitter, retry only on connect error / timeout / 429 / 5xx, never on other 4xx, never mid-stream.
- An `LlmError` type whose `to_string()` output is compatible with the existing strings the frontend already displays, so this is a transport refactor, not a UX change — except for the one intentional fix (correct timeout value in the message).
- Keep provider modules focused on request construction and response parsing only.

**Non-Goals:**
- No `trait LlmProvider` abstraction (see proposal.md "What Changes" for the evidence-based reasoning: Claude's request/response shape and headers differ from the four OpenAI-compatible providers, and the four model-listing response shapes are four different structs already).
- No change to retry/timeout behavior for `pull_ollama_model` (long-running download) or `delete_ollama_model` — they adopt the shared client for pooling only.
- No streaming support is added or changed; none of the five providers stream today.
- No change to the debug-logging behavior in `summary/debug_log.rs` (`llm-debug-logging` capability) — `send_with_retry` calls happen underneath `generate_summary`'s existing debug-log write points, and only the final (successful or exhausted) outcome is logged, not each individual retry attempt.

## Decisions

### D1: A minimal `llm` module, not a provider trait

New `frontend/src-tauri/src/llm/{mod.rs, client.rs, error.rs}`, registered as `pub mod llm;` in `lib.rs`. `client.rs` exposes:
- `pub fn shared_client() -> &'static reqwest::Client` — built once via `once_cell::sync::Lazy`, with `.connect_timeout(Duration::from_secs(10))`, `.timeout(Duration::from_secs(300))` (matching the current `generate_summary` timeout so summary behavior does not silently change) and `.user_agent(concat!("meetily/", env!("CARGO_PKG_VERSION")))`.
- `pub struct RetryPolicy { max_retries: u32, base_backoff: Duration, ... }` with a sensible default (`max_retries: 2`, matching Ollama's current `MAX_RETRIES` so the change is not a big behavioral jump) and a shorter policy usable for model-listing calls (which already have their own tight 3-5s per-request timeouts today).
- `pub async fn send_with_retry(build_request: impl Fn() -> RequestBuilder, policy: &RetryPolicy) -> Result<Response, LlmError>` — takes a closure that rebuilds the request (not a single `RequestBuilder`), because a `RequestBuilder` is consumed by `.send()` and cannot be reused across attempts; the closure re-creates an equivalent request each attempt from data the caller already owns (URL, headers, JSON body), which is what makes "idempotent requests only" true by construction — the caller can only pass a closure it is safe to call more than once.

`error.rs` defines:
```
pub enum LlmError {
    Timeout { seconds: u64 },
    Connect(String),
    Http { status: u16, body: String },
    AuthFailed { status: u16, body: String },
    Decode(String),
    Cancelled,
}
```
with a `Display` impl that reproduces today's strings: `Timeout` → `"LLM request timed out after {seconds} seconds"` (now with the *real* configured value), `Connect`/other send failure → `"Failed to send request to LLM: {msg}"`, `Http`/`AuthFailed` → `"LLM API request failed: {body}"`. A `From<LlmError> for String` (or `.to_string()` used at the call boundary) keeps every call site's existing `Result<T, String>` IPC surface unchanged.

- Alternative considered: put this in `summary/http.rs`. Rejected because the five provider modules (not just `summary/`) need it, and nesting a cross-cutting utility under `summary/` would be a layering inversion (provider modules would depend on `summary::http`).

### D2: Retry classification lives in one function, keyed off the actual response/error, not string-matching

`send_with_retry` classifies each attempt's outcome itself: a `reqwest::Error` where `.is_timeout()` or `.is_connect()` is true → retryable; a successful `Response` with `status() == 429` or `status().is_server_error()` → retryable; `status() == 401 || status() == 403` → `LlmError::AuthFailed`, returned immediately, no retry; any other 4xx → `LlmError::Http`, returned immediately, no retry.

- Why: this replaces Ollama's current `e.contains("Invalid endpoint") || e.contains("404")` string-matching (`ollama.rs:141`), which is fragile and, as noted in Context, can retry a 401/403-shaped message that doesn't happen to contain "404". Classifying on the actual `StatusCode`/`reqwest::Error` methods is exact and provider-independent.
- Alternative considered: let each provider module classify its own errors. Rejected — the whole point of this change is that this logic is identical across providers (HTTP status codes mean the same thing everywhere); duplicating it five times is the bug we're removing.

### D3: Exponential backoff with jitter, bounded attempts

Backoff for attempt `n` (0-indexed retry) is `base_backoff * 2^n + random_jitter(0..=base_backoff)`, capped at a max delay (e.g. 5s) so a misconfigured `max_retries` cannot produce multi-minute waits. `max_retries: 2` as the default (3 total attempts), matching today's Ollama model-list behavior so this is not a bigger behavioral jump than necessary; the retry ceiling is a named constant in `client.rs`, not per-provider, so there is exactly one place to tune it.

- Jitter source: reuse a lightweight source already reachable transitively (check at implementation time whether `fastrand`/`rand` is already pulled in by an existing dependency such as `criterion` or `tokio`'s dev-features; if not, a simple time-based pseudo-jitter — e.g. `(Instant::now().elapsed().subsec_nanos() % base_backoff_ms)` — avoids adding a new runtime dependency for something this small).
- Alternative considered: no jitter. Rejected because multiple chunks of the same summary session retrying in lockstep against the same provider would otherwise create synchronized retry bursts.

### D4: Migrate call sites, keep provider-specific logic where it is

- `summary/service.rs:535`: delete `let client = reqwest::Client::new();`; pass `llm::shared_client()` into `generate_meeting_summary`/`generate_summary` instead. `generate_summary`'s own `.send()` call is replaced with `llm::send_with_retry(|| client.post(&api_url).headers(headers.clone()).json(&request_body), &RetryPolicy::default())`, with the cancellation `tokio::select!` wrapped around the whole `send_with_retry` future (unchanged pattern, just around the retrying call instead of a single `.send()`).
- `ollama/ollama.rs`: `get_models_via_http_with_retry` (126-157) is deleted; `get_models_via_http_async` (162) becomes a single request built through `llm::shared_client()` and `llm::send_with_retry`, with a shorter `RetryPolicy` (model-listing already has a 3s per-request timeout and a 5s overall `tokio::time::timeout` wrapper at the call site in `get_ollama_models`, so the retry ceiling here stays small to fit inside that 5s budget — likely `max_retries: 1`).
- `openai/openai.rs:130`, `anthropic/anthropic.rs:100`, `groq/groq.rs:98`: replace `reqwest::Client::new()` with `llm::shared_client()`, wrap `.send()` in `llm::send_with_retry`; the existing "fall back to a hardcoded model list on any failure" behavior is preserved — it now triggers after `send_with_retry` exhausts retries instead of after a single failed call.
- `openrouter/openrouter.rs:43`: convert `get_openrouter_models` from `pub fn` (sync) to `pub async fn`, switch from `reqwest::blocking::Client` to `llm::shared_client()` + `llm::send_with_retry`. This is called out explicitly because it changes the function's async-ness (a Tauri `#[command]` can be either; callers use `invoke()` either way, so the IPC surface is unaffected) and removes a blocking call from a Tauri command thread — a real (positive) behavioral side effect worth flagging even though it's not spec-visible.
- `ollama/ollama.rs:287` (`pull_ollama_model`) and `:458` (`delete_ollama_model`): `Client::new()` → `llm::shared_client()`, no retry wrapper (see proposal.md Impact/Out of scope).

### D5: Testing with `wiremock`

Add `wiremock = "0.5"` under `[dev-dependencies]`. Unit tests in `llm/client.rs` (or a `tests` submodule) start a local `wiremock::MockServer`, point `send_with_retry` at it, and assert:
- a `503` followed by a `200` results in the `200` body being returned (retry-on-5xx path);
- a `401` is returned immediately with exactly one request received by the mock server (no-retry-on-auth path);
- a mock server that never responds within a short configured timeout causes `send_with_retry` to return `LlmError::Timeout` after the bounded attempts, and the elapsed wall time is asserted to be within a small multiple of the configured timeout (bounding the backoff, not exact-matching it, to avoid flaky timing assertions).

- Alternative considered: `mockito` — Rejected: `mockito`'s server is process-global/blocking-server-based in the version compatible with this MSRV, which is less ergonomic for concurrent async tests than `wiremock`'s per-test `MockServer`. `httpmock` — Rejected: broadly equivalent to `wiremock`; `wiremock` is chosen for its async-first API matching this codebase's `tokio` usage throughout.

## Risks / Trade-offs

- **Retrying a 5xx that is actually a permanent provider-side error** (e.g. a persistently broken CustomOpenAI endpoint) → the bounded `max_retries` (default 2) caps the added latency to a few seconds before the existing error surfaces; this is strictly better than today's single-attempt failure for the common transient case, at the cost of a few seconds of extra latency in the permanent-failure case.
- **`send_with_retry`'s closure-based API is a larger refactor at each call site than swapping `Client::new()` for a shared static** → mitigated by doing it once per call site with a small, consistent pattern (build request → `send_with_retry` → same status/parsing code as before).
- **Timeout value is now correct in the message, which changes user-visible text** → this is an intentional, minor, positive behavior fix (the message was simply wrong before); called out explicitly in proposal.md so it isn't mistaken for scope creep.
- **New dev-dependency (`wiremock`)** → dev-only, does not affect the shipped binary; accepted as the cost of testing retry/timeout logic against real HTTP semantics instead of hand-rolled fakes.
- **`openrouter.rs`'s sync→async conversion touches its call site(s)** → confirmed at implementation time to be limited to its own `#[command]` registration in `lib.rs`'s `generate_handler!`; Tauri commands may be sync or async interchangeably from the frontend's perspective.

## Migration Plan

- Purely additive/internal: no IPC signature changes, no database migration, no frontend changes required. Land the `llm` module and its tests first, then migrate call sites one module at a time (`summary` → `ollama` → `openai`/`anthropic`/`groq` → `openrouter`), each independently buildable and testable via `cargo check -p meetily` / `cargo test -p meetily --lib llm`.
- Rollback: revert the call-site commits; the `llm` module itself has no external state to unwind.

## Open Questions

- Should the model-listing `RetryPolicy` (shorter, to fit inside the existing 5s `tokio::time::timeout` in `get_ollama_models`) be a second named constant, or should the outer per-command timeout wrappers (`get_ollama_models`'s 5s, each model-list command's implicit per-request 3-5s) be widened slightly so the default `RetryPolicy` can be used everywhere? Left as an implementation-time call; either choice satisfies the spec's "bounded attempts" and "time-bounded" requirements.
- Is `max_retries: 2` the right default long-term, or should it be configurable per-provider later (e.g. a slower local Ollama vs. a paid cloud API)? Not needed for this change; the spec only requires *a* bounded retry policy exists.
