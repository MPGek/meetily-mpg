use crate::llm::error::LlmError;
use once_cell::sync::Lazy;
use std::time::Duration;

/// Matches `generate_summary`'s previous `REQUEST_TIMEOUT_DURATION`, so
/// migrating it to the shared client does not silently change its timeout.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

static SHARED_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .user_agent(concat!("meetily/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build the shared LLM reqwest::Client")
});

/// One process-wide, connection-pooled HTTP client for every outbound call
/// to an LLM provider (summary generation and model listing).
pub fn shared_client() -> &'static reqwest::Client {
    &SHARED_CLIENT
}

/// Bounded retry policy for `send_with_retry`. `request_timeout_secs` is
/// informational only (used to report the real configured value in a
/// `LlmError::Timeout` message) — the actual per-request timeout, if a call
/// site needs one shorter than the client's default, is still set on the
/// `RequestBuilder` inside the `build_request` closure exactly as before.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_backoff: Duration,
    pub max_backoff: Duration,
    pub request_timeout_secs: u64,
}

impl Default for RetryPolicy {
    /// `max_retries: 2` matches Ollama's current `MAX_RETRIES` so adopting
    /// this shared policy is not a bigger behavioral jump than necessary.
    fn default() -> Self {
        Self {
            max_retries: 2,
            base_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(5),
            request_timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

impl RetryPolicy {
    /// A tighter policy for model-listing call sites, which already run
    /// under their own short per-request timeout (a few seconds) and, for
    /// Ollama, an outer 5s `tokio::time::timeout` wrapper at the call site —
    /// fewer retries and a shorter backoff keep the total bounded within
    /// that existing budget.
    pub fn short() -> Self {
        Self {
            max_retries: 1,
            base_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(2),
            request_timeout_secs: 5,
        }
    }

    /// Overrides the informational timeout value reported in a
    /// `LlmError::Timeout` message, to match a call site's own shorter
    /// per-request `.timeout(...)`.
    pub fn with_timeout_secs(mut self, seconds: u64) -> Self {
        self.request_timeout_secs = seconds;
        self
    }

    fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let base_ms = self.base_backoff.as_millis() as u64;
        let exp_ms = base_ms.saturating_mul(1u64 << attempt.min(16));
        let jitter_ms = if base_ms == 0 { 0 } else { fastrand::u64(0..=base_ms) };
        Duration::from_millis(exp_ms.saturating_add(jitter_ms)).min(self.max_backoff)
    }
}

/// Sends a request built by `build_request`, retrying on a connect error,
/// a timeout, HTTP 429, or HTTP 5xx, with exponential backoff and jitter
/// bounded by `policy`. HTTP 401/403 return `LlmError::AuthFailed`
/// immediately, with no retry; any other HTTP 4xx returns `LlmError::Http`
/// immediately. `build_request` is a closure (not a single `RequestBuilder`,
/// which `.send()` consumes) so it can be called again on each retry —
/// callers can only pass a closure safe to call more than once, which is
/// what makes "idempotent requests only" true by construction.
pub async fn send_with_retry(
    build_request: impl Fn() -> reqwest::RequestBuilder,
    policy: &RetryPolicy,
) -> Result<reqwest::Response, LlmError> {
    let mut attempt = 0u32;
    loop {
        match build_request().send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    return Ok(response);
                }

                let status_u16 = status.as_u16();
                if status_u16 == 401 || status_u16 == 403 {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(LlmError::AuthFailed {
                        status: status_u16,
                        body,
                    });
                }

                let retryable = status_u16 == 429 || status.is_server_error();
                if retryable && attempt < policy.max_retries {
                    tokio::time::sleep(policy.backoff_for_attempt(attempt)).await;
                    attempt += 1;
                    continue;
                }

                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());
                return Err(LlmError::Http {
                    status: status_u16,
                    body,
                });
            }
            Err(e) => {
                let retryable = e.is_timeout() || e.is_connect();
                if retryable && attempt < policy.max_retries {
                    tokio::time::sleep(policy.backoff_for_attempt(attempt)).await;
                    attempt += 1;
                    continue;
                }

                if e.is_timeout() {
                    return Err(LlmError::Timeout {
                        seconds: policy.request_timeout_secs,
                    });
                }
                return Err(LlmError::Connect(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn retries_a_503_and_returns_the_eventual_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let response = send_with_retry(
            || shared_client().get(&url),
            &RetryPolicy::default(),
        )
        .await
        .expect("503 then 200 should eventually succeed");

        assert_eq!(response.text().await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn a_401_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .expect(1)
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let err = send_with_retry(|| shared_client().get(&url), &RetryPolicy::default())
            .await
            .expect_err("401 must not succeed");

        match err {
            LlmError::AuthFailed { status, .. } => assert_eq!(status, 401),
            other => panic!("expected AuthFailed, got {:?}", other),
        }

        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "401 must not be retried"
        );
    }

    #[tokio::test]
    async fn a_response_that_never_arrives_in_time_returns_timeout() {
        let server = MockServer::start().await;
        // Delayed well beyond the per-request timeout below, on every attempt.
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;

        let url = format!("{}/v1/models", server.uri());
        let policy = RetryPolicy {
            max_retries: 1,
            base_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(50),
            request_timeout_secs: 0, // informational; actual timeout set below
        };

        let start = std::time::Instant::now();
        let err = send_with_retry(
            || shared_client().get(&url).timeout(Duration::from_millis(100)),
            &policy,
        )
        .await
        .expect_err("a request that never responds in time must fail");
        let elapsed = start.elapsed();

        assert!(matches!(err, LlmError::Timeout { .. }), "expected Timeout, got {:?}", err);
        // Bounding, not exact-matching: 2 attempts * (~100ms timeout + <=60ms
        // backoff) should stay well under 2s (a real response would have
        // taken, since it's delayed 2s).
        assert!(
            elapsed < Duration::from_secs(2),
            "elapsed {:?} suggests the bounded attempts were not actually bounded",
            elapsed
        );
    }

    #[test]
    fn shared_client_returns_the_same_instance_every_call() {
        let a = shared_client() as *const reqwest::Client;
        let b = shared_client() as *const reqwest::Client;
        assert_eq!(a, b, "shared_client() must return the same static instance");
    }

    #[test]
    fn default_policy_matches_ollamas_current_retry_count() {
        assert_eq!(RetryPolicy::default().max_retries, 2);
    }

    #[test]
    fn with_timeout_secs_overrides_the_reported_timeout() {
        let policy = RetryPolicy::short().with_timeout_secs(3);
        assert_eq!(policy.request_timeout_secs, 3);
    }
}
