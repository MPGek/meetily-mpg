# Spec Delta

## Purpose

Define provider-agnostic HTTP resilience — connection reuse, bounded retry on transient failures, no retry on authentication/validation failures, and request timeouts — for every outbound call this application makes to an LLM provider (Ollama, OpenAI, Anthropic/Claude, Groq, OpenRouter, CustomOpenAI), whether the call generates a summary or lists available models.

## ADDED Requirements

### Requirement: Transient provider failures are retried

When a call to an LLM provider fails with a connection error, a request timeout, HTTP 429, or an HTTP 5xx response, the system SHALL retry the request up to a bounded maximum number of attempts before surfacing an error to the caller, waiting between attempts with an increasing (exponential) backoff that includes random jitter.

#### Scenario: Provider returns 503 then succeeds
- **WHEN** an LLM provider responds with HTTP 503 on the first attempt and a successful response on a subsequent attempt, within the bounded number of attempts
- **THEN** the system SHALL retry and return the successful result to the caller without surfacing the intermediate failure

#### Scenario: Provider is unreachable
- **WHEN** the underlying connection to the provider cannot be established (a connect error)
- **THEN** the system SHALL retry up to the bounded maximum before surfacing an error

#### Scenario: Retries are exhausted
- **WHEN** every attempt up to the bounded maximum fails with a transient error
- **THEN** the system SHALL surface a single error to the caller describing the failure, without retrying further

### Requirement: Authentication and validation failures are not retried

When a call to an LLM provider fails with an HTTP 401 or HTTP 403 response, the system SHALL NOT retry the request and SHALL surface the failure to the caller immediately. Other non-transient HTTP 4xx responses (e.g. 400, 404) SHALL likewise not be retried.

#### Scenario: Invalid API key
- **WHEN** an LLM provider responds with HTTP 401 or HTTP 403
- **THEN** the system SHALL surface the failure immediately, with no retry attempt

#### Scenario: Malformed request
- **WHEN** an LLM provider responds with HTTP 400 or HTTP 404
- **THEN** the system SHALL surface the failure immediately, with no retry attempt

### Requirement: Provider requests are time-bounded

Every call to an LLM provider SHALL be subject to a request timeout. If a response is not received within the configured timeout, the attempt SHALL be treated as a transient failure eligible for retry under the retry requirement above, and the resulting error message SHALL state the actual configured timeout duration.

#### Scenario: Provider does not respond in time
- **WHEN** an LLM provider does not return a response within the configured timeout
- **THEN** the attempt SHALL be treated as a transient failure and SHALL be eligible for retry
- **AND** if all retries are exhausted, the surfaced error message SHALL report the actual configured timeout value

### Requirement: In-progress streamed responses are not retried

If a provider response has begun streaming data back to the caller, the system SHALL NOT automatically retry that request on a subsequent failure of the same stream, since part of the response may already have been consumed or surfaced.

#### Scenario: Stream fails partway through
- **WHEN** a provider response stream fails after some data has already been delivered to the caller
- **THEN** the system SHALL surface the failure without re-issuing the request automatically

### Requirement: Provider connections are reused across calls

The system SHALL reuse a single underlying HTTP client configuration (connection pool) across calls to LLM providers within a process, instead of establishing a new connection setup for every call.

#### Scenario: Consecutive calls to the same provider reuse a connection
- **WHEN** two calls are made to the same LLM provider endpoint in quick succession
- **THEN** the underlying HTTP client configuration SHALL be shared between them rather than newly constructed per call
