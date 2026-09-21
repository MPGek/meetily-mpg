//! Shared HTTP transport for outbound calls to LLM providers (summary
//! generation and model listing): a pooled client, a bounded-retry helper,
//! and a provider-agnostic error type. See `openspec/specs/llm-provider-resilience`.

pub mod client;
pub mod error;

pub use client::{send_with_retry, shared_client, RetryPolicy};
pub use error::LlmError;
