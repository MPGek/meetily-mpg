use std::fmt;

/// Provider-agnostic HTTP error for outbound LLM calls. `Display` reproduces
/// the strings call sites already surfaced to the frontend before this
/// module existed, so migrating a call site to `send_with_retry` is a
/// transport refactor, not a user-visible wording change (the one
/// intentional exception: `Timeout` now reports the real configured value
/// instead of the stale hardcoded "60 seconds").
#[derive(Debug, Clone)]
pub enum LlmError {
    Timeout { seconds: u64 },
    Connect(String),
    Http { status: u16, body: String },
    AuthFailed { status: u16, body: String },
    Decode(String),
    Cancelled,
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::Timeout { seconds } => {
                write!(f, "LLM request timed out after {} seconds", seconds)
            }
            LlmError::Connect(msg) => write!(f, "Failed to send request to LLM: {}", msg),
            LlmError::Http { body, .. } => write!(f, "LLM API request failed: {}", body),
            LlmError::AuthFailed { body, .. } => write!(f, "LLM API request failed: {}", body),
            LlmError::Decode(msg) => write!(f, "Failed to parse LLM response: {}", msg),
            LlmError::Cancelled => write!(f, "Summary generation was cancelled"),
        }
    }
}

impl std::error::Error for LlmError {}

impl From<LlmError> for String {
    fn from(err: LlmError) -> Self {
        err.to_string()
    }
}
