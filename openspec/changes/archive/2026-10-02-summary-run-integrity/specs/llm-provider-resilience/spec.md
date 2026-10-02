## MODIFIED Requirements

### Requirement: Authentication and validation failures are not retried

When a call to an LLM provider fails with an HTTP 401 or HTTP 403 response, the system SHALL NOT retry the request and SHALL surface the failure to the caller immediately. Other non-transient HTTP 4xx responses (e.g. 400, 404) SHALL likewise not be retried. The single Ollama compatibility re-send, which sends a changed request body without `reasoning_effort` after Ollama rejects that field, is a different request and not a retry of the failed one.

#### Scenario: Invalid API key
- **WHEN** an LLM provider responds with HTTP 401 or HTTP 403
- **THEN** the system SHALL surface the failure immediately, with no retry attempt

#### Scenario: Malformed request
- **WHEN** an LLM provider responds with HTTP 400 or HTTP 404
- **THEN** the system SHALL surface the failure immediately, with no retry attempt

#### Scenario: Ollama compatibility re-send is bounded
- **WHEN** Ollama rejects `reasoning_effort` with HTTP 400 and the compatibility re-send without that field also returns HTTP 400
- **THEN** the system SHALL surface the second failure, having sent exactly two requests in total
