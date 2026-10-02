use crate::summary::debug_log::{self, DebugLogEntry, DebugLogResult};
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

// Generic structure for OpenAI-compatible API chat messages
#[derive(Debug, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// Generic structure for OpenAI-compatible API chat requests
#[derive(Debug, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// `Some("none")` only for Ollama, to switch reasoning models' thinking off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<&'static str>,
}

// Generic structure for OpenAI-compatible API chat responses
#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    pub message: MessageContent,
}

/// Reasoning models can return `"content": null` with their text in
/// `reasoning` / `reasoning_content`. Those fields are parsed so the response
/// does not fail, but they are never used as summary text.
#[derive(Deserialize, Debug)]
pub struct MessageContent {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub reasoning: Option<String>,
    #[serde(default)]
    pub reasoning_content: Option<String>,
}

impl MessageContent {
    fn visible_text(&self) -> String {
        self.content
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    fn discarded_reasoning_len(&self) -> usize {
        [&self.reasoning, &self.reasoning_content]
            .iter()
            .filter_map(|field| field.as_deref())
            .map(|text| text.trim().len())
            .sum()
    }
}

/// True when Ollama rejected the `reasoning_effort` field (older Ollama, or a
/// model without thinking support). Deliberately broad: a false positive costs
/// one extra request that fails the same way.
fn ollama_rejected_reasoning_effort(err: &crate::llm::LlmError) -> bool {
    match err {
        crate::llm::LlmError::Http {
            status: 400 | 422,
            body,
        } => {
            let body = body.to_lowercase();
            body.contains("reasoning_effort") || body.contains("think")
        }
        _ => false,
    }
}

fn build_chat_request(
    provider: &LLMProvider,
    model_name: &str,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
) -> ChatRequest {
    // For CustomOpenAI, apply optional parameters if provided
    let (max_tokens_val, temperature_val, top_p_val) = if provider == &LLMProvider::CustomOpenAI {
        (max_tokens, temperature, top_p)
    } else {
        (None, None, None)
    };

    ChatRequest {
        model: model_name.to_string(),
        messages: vec![
            ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            },
        ],
        max_tokens: max_tokens_val,
        temperature: temperature_val,
        top_p: top_p_val,
        reasoning_effort: (provider == &LLMProvider::Ollama).then_some("none"),
    }
}

// Claude-specific request structure
#[derive(Debug, Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<ChatMessage>,
}

// Claude-specific response structure
#[derive(Deserialize, Debug)]
pub struct ClaudeChatResponse {
    pub content: Vec<ClaudeChatContent>,
}

#[derive(Deserialize, Debug)]
pub struct ClaudeChatContent {
    #[serde(rename = "type")]
    pub kind: String,
    // Only `text` blocks carry this field. Models that enable thinking by
    // default (Sonnet 5, Opus 5) also return `thinking` blocks, which don't.
    pub text: Option<String>,
}

impl ClaudeChatResponse {
    /// First `text` block. With thinking enabled the leading block is a
    /// `thinking` block, so `content[0]` is not necessarily the answer.
    fn first_text(&self) -> Option<&str> {
        self.content
            .iter()
            .filter(|block| block.kind == "text")
            .find_map(|block| block.text.as_deref())
    }
}

/// LLM Provider enumeration for multi-provider support
#[derive(Debug, Clone, PartialEq)]
pub enum LLMProvider {
    OpenAI,
    Claude,
    Groq,
    Ollama,
    OpenRouter,
    BuiltInAI,
    CustomOpenAI,
}

impl LLMProvider {
    /// Parse provider from string (case-insensitive)
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Self::OpenAI),
            "claude" => Ok(Self::Claude),
            "groq" => Ok(Self::Groq),
            "ollama" => Ok(Self::Ollama),
            "openrouter" => Ok(Self::OpenRouter),
            "builtin-ai" | "local-llama" | "localllama" => Ok(Self::BuiltInAI),
            "custom-openai" => Ok(Self::CustomOpenAI),
            _ => Err(format!("Unsupported LLM provider: {}", s)),
        }
    }
}

/// Generates a summary using the specified LLM provider
///
/// # Arguments
/// * `client` - Reqwest HTTP client (reused for performance)
/// * `provider` - The LLM provider to use
/// * `model_name` - The specific model to use (e.g., "gpt-4", "claude-3-opus")
/// * `api_key` - API key for the provider (not needed for Ollama)
/// * `system_prompt` - System instructions for the LLM
/// * `user_prompt` - User query/content to process
/// * `ollama_endpoint` - Optional custom Ollama endpoint (defaults to localhost:11434)
/// * `custom_openai_endpoint` - Optional custom OpenAI-compatible endpoint
/// * `max_tokens` - Optional max tokens (for CustomOpenAI provider)
/// * `temperature` - Optional temperature (for CustomOpenAI provider)
/// * `top_p` - Optional top_p (for CustomOpenAI provider)
/// * `app_data_dir` - Optional app data directory (for BuiltInAI provider)
/// * `debug_log_dir` - Optional directory for debug logs (LLM interaction payloads)
/// * `cancellation_token` - Optional token to cancel the request
///
/// # Returns
/// The generated summary text or an error message
#[allow(clippy::too_many_arguments)] // 14 params; a params struct would change every call site; no owning change yet
pub async fn generate_summary(
    client: &Client,
    provider: &LLMProvider,
    model_name: &str,
    api_key: &str,
    system_prompt: &str,
    user_prompt: &str,
    ollama_endpoint: Option<&str>,
    custom_openai_endpoint: Option<&str>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    app_data_dir: Option<&PathBuf>,
    debug_log_dir: Option<PathBuf>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String, String> {
    // Check if cancelled before starting
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err("Summary generation was cancelled".to_string());
        }
    }

    let call_start = Instant::now();
    let iteration = debug_log::next_iteration();
    let start_timestamp = debug_log::iso_timestamp();

    // Handle BuiltInAI provider separately (uses local sidecar, no HTTP API)
    if provider == &LLMProvider::BuiltInAI {
        let app_data_dir = app_data_dir
            .ok_or_else(|| "app_data_dir is required for BuiltInAI provider".to_string())?;

        return crate::summary::summary_engine::generate_with_builtin(
            app_data_dir,
            model_name,
            system_prompt,
            user_prompt,
            cancellation_token,
            debug_log_dir,
        )
        .await
        .map_err(|e| e.to_string());
    }

    let (api_url, mut headers) = match provider {
        LLMProvider::OpenAI => (
            "https://api.openai.com/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Groq => (
            "https://api.groq.com/openai/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::OpenRouter => (
            "https://openrouter.ai/api/v1/chat/completions".to_string(),
            header::HeaderMap::new(),
        ),
        LLMProvider::Ollama => {
            let host = ollama_endpoint
                .map(|s| s.to_string())
                .unwrap_or_else(|| "http://localhost:11434".to_string());
            (
                format!("{}/v1/chat/completions", host),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::CustomOpenAI => {
            let endpoint = custom_openai_endpoint
                .ok_or_else(|| "Custom OpenAI endpoint not configured".to_string())?;
            (
                format!("{}/chat/completions", endpoint.trim_end_matches('/')),
                header::HeaderMap::new(),
            )
        }
        LLMProvider::Claude => {
            let mut header_map = header::HeaderMap::new();
            header_map.insert(
                "x-api-key",
                api_key
                    .parse()
                    .map_err(|_| "Invalid API key format".to_string())?,
            );
            header_map.insert(
                "anthropic-version",
                "2023-06-01"
                    .parse()
                    .map_err(|_| "Invalid anthropic version".to_string())?,
            );
            (
                "https://api.anthropic.com/v1/messages".to_string(),
                header_map,
            )
        }
        LLMProvider::BuiltInAI => {
            // This case is handled earlier with early returns
            unreachable!("BuiltInAI is handled before this match statement")
        }
    };

    // Add authorization header for non-Claude providers
    if provider != &LLMProvider::Claude {
        headers.insert(
            header::AUTHORIZATION,
            format!("Bearer {}", api_key)
                .parse()
                .map_err(|_| "Invalid authorization header".to_string())?,
        );
    }
    headers.insert(
        header::CONTENT_TYPE,
        "application/json"
            .parse()
            .map_err(|_| "Invalid content type".to_string())?,
    );

    // Build request body based on provider
    let request_body = if provider != &LLMProvider::Claude {
        serde_json::json!(build_chat_request(
            provider,
            model_name,
            system_prompt,
            user_prompt,
            max_tokens,
            temperature,
            top_p,
        ))
    } else {
        serde_json::json!(ClaudeRequest {
            system: system_prompt.to_string(),
            model: model_name.to_string(),
            // Shared budget: on models with thinking enabled by default this
            // covers thinking tokens as well as the answer.
            max_tokens: 8192,
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }]
        })
    };

    let provider_name_str = provider_name(provider);
    info!(
        "🐞 LLM Request to {}: model={}",
        provider_name_str, model_name
    );

    let debug_entry = debug_log_dir.as_ref().map(|_| DebugLogEntry {
        start_timestamp: start_timestamp.clone(),
        provider: provider_name_str.to_string(),
        model: model_name.to_string(),
        request_json: request_body.clone(),
        iteration,
    });

    // Send request (bounded retry on transient failures, no retry on auth
    // failures) and race it against cancellation, exactly as before — now
    // around the whole retrying future instead of a single `.send()`. A
    // successful `Ok(response)` from `send_with_retry` is already a
    // successful HTTP status; failed statuses come back as `Err(LlmError)`.
    //
    // Ollama compatibility: if Ollama rejects `reasoning_effort`, send the
    // same request once more without it. That is a different request, not a
    // transport retry, and it is never repeated.
    let retry_policy = crate::llm::RetryPolicy::default();
    let request_future = async {
        let first = crate::llm::send_with_retry(
            || {
                client
                    .post(&api_url)
                    .headers(headers.clone())
                    .json(&request_body)
            },
            &retry_policy,
        )
        .await;
        match first {
            Err(e) if provider == &LLMProvider::Ollama && ollama_rejected_reasoning_effort(&e) => {
                warn!(
                    "Ollama rejected reasoning_effort ({}); re-sending once without it",
                    e
                );
                let mut fallback_body = request_body.clone();
                if let Some(fields) = fallback_body.as_object_mut() {
                    fields.remove("reasoning_effort");
                }
                crate::llm::send_with_retry(
                    || {
                        client
                            .post(&api_url)
                            .headers(headers.clone())
                            .json(&fallback_body)
                    },
                    &retry_policy,
                )
                .await
            }
            other => other,
        }
    };

    let log_error = |err_msg: &str| {
        if let (Some(ref log_dir), Some(ref entry)) = (&debug_log_dir, &debug_entry) {
            let result = DebugLogResult::Error {
                end_timestamp: debug_log::iso_timestamp(),
                elapsed_secs: debug_log::elapsed_secs(&call_start),
                error_message: err_msg.to_string(),
                partial_response: None,
            };
            debug_log::write_debug_log(log_dir, entry, &result);
        }
    };

    // Use tokio::select to race between cancellation and request completion
    let response = if let Some(token) = cancellation_token {
        tokio::select! {
            result = request_future => {
                match result {
                    Ok(resp) => resp,
                    Err(e) => {
                        let err_msg = e.to_string();
                        log_error(&err_msg);
                        return Err(err_msg);
                    }
                }
            }
            _ = token.cancelled() => {
                return Err("Summary generation was cancelled".to_string());
            }
        }
    } else {
        match request_future.await {
            Ok(resp) => resp,
            Err(e) => {
                let err_msg = e.to_string();
                log_error(&err_msg);
                return Err(err_msg);
            }
        }
    };

    let status_code = response.status().as_u16();

    // Parse response based on provider
    let result: Result<String, String> = if provider == &LLMProvider::Claude {
        let chat_response = response
            .json::<ClaudeChatResponse>()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        info!("🐞 LLM Response received from Claude");

        let content = chat_response
            .first_text()
            .ok_or("No text content in LLM response")?
            .trim()
            .to_string();
        Ok(content)
    } else {
        let chat_response = response
            .json::<ChatResponse>()
            .await
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        info!("🐞 LLM Response received from {}", provider_name_str);

        let message = &chat_response
            .choices
            .first()
            .ok_or("No content in LLM response")?
            .message;
        let discarded = message.discarded_reasoning_len();
        if discarded > 0 {
            info!(
                "Discarded {} chars of separate reasoning from {} response",
                discarded, provider_name_str
            );
        }
        Ok(message.visible_text())
    };

    if let (Some(ref log_dir), Some(ref entry)) = (&debug_log_dir, &debug_entry) {
        match &result {
            Ok(text) => {
                let log_result = DebugLogResult::Success {
                    end_timestamp: debug_log::iso_timestamp(),
                    elapsed_secs: debug_log::elapsed_secs(&call_start),
                    status_code,
                    response_body: text.clone(),
                };
                debug_log::write_debug_log(log_dir, entry, &log_result);
            }
            Err(err) => {
                let log_result = DebugLogResult::Error {
                    end_timestamp: debug_log::iso_timestamp(),
                    elapsed_secs: debug_log::elapsed_secs(&call_start),
                    error_message: err.clone(),
                    partial_response: None,
                };
                debug_log::write_debug_log(log_dir, entry, &log_result);
            }
        }
    }

    result
}

/// Helper function to get provider name for logging
fn provider_name(provider: &LLMProvider) -> &str {
    match provider {
        LLMProvider::OpenAI => "OpenAI",
        LLMProvider::Claude => "Claude",
        LLMProvider::Groq => "Groq",
        LLMProvider::Ollama => "Ollama",
        LLMProvider::BuiltInAI => "Built-in AI",
        LLMProvider::OpenRouter => "OpenRouter",
        LLMProvider::CustomOpenAI => "Custom OpenAI",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_response_skips_leading_thinking_block() {
        // Shape returned by models with thinking enabled by default,
        // e.g. claude-sonnet-5 / claude-opus-5.
        let response: ClaudeChatResponse = serde_json::from_value(json!({
            "content": [
                {"type": "thinking", "thinking": "", "signature": "abc"},
                {"type": "text", "text": "Meeting summary."}
            ]
        }))
        .expect("thinking blocks must not fail deserialization");

        assert_eq!(response.first_text(), Some("Meeting summary."));
    }

    #[test]
    fn claude_response_reads_plain_text_block() {
        let response: ClaudeChatResponse = serde_json::from_value(json!({
            "content": [{"type": "text", "text": "Meeting summary."}]
        }))
        .unwrap();

        assert_eq!(response.first_text(), Some("Meeting summary."));
    }

    #[test]
    fn claude_response_without_text_block_returns_none() {
        let response: ClaudeChatResponse = serde_json::from_value(json!({
            "content": [{"type": "thinking", "thinking": "", "signature": "abc"}]
        }))
        .unwrap();

        assert_eq!(response.first_text(), None);
    }

    #[test]
    fn claude_response_ignores_non_text_block_carrying_text() {
        let response: ClaudeChatResponse = serde_json::from_value(json!({
            "content": [
                {"type": "server_tool_result", "text": "x"},
                {"type": "text", "text": "Meeting summary."}
            ]
        }))
        .unwrap();

        assert_eq!(response.first_text(), Some("Meeting summary."));
    }

    // Ollama reasoning compatibility ------------------------------------------

    use crate::llm::LlmError;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn body_for(provider: &LLMProvider) -> serde_json::Value {
        serde_json::to_value(build_chat_request(
            provider, "m", "sys", "user", None, None, None,
        ))
        .unwrap()
    }

    #[test]
    fn only_ollama_body_carries_reasoning_effort_none() {
        assert_eq!(body_for(&LLMProvider::Ollama)["reasoning_effort"], "none");
        for provider in [
            LLMProvider::OpenAI,
            LLMProvider::Groq,
            LLMProvider::OpenRouter,
            LLMProvider::CustomOpenAI,
        ] {
            assert!(
                body_for(&provider).get("reasoning_effort").is_none(),
                "{provider:?} body must not carry reasoning_effort"
            );
        }
    }

    #[test]
    fn null_or_missing_content_parses_to_empty_text() {
        let null_content: ChatResponse = serde_json::from_value(json!({
            "choices": [{"message": {"content": null, "reasoning_content": "x"}}]
        }))
        .unwrap();
        assert_eq!(null_content.choices[0].message.visible_text(), "");

        let no_content: ChatResponse = serde_json::from_value(json!({
            "choices": [{"message": {"role": "assistant", "reasoning": "x"}}]
        }))
        .unwrap();
        assert_eq!(no_content.choices[0].message.visible_text(), "");
        assert_eq!(no_content.choices[0].message.discarded_reasoning_len(), 1);
    }

    #[test]
    fn content_with_reasoning_yields_only_content() {
        let response: ChatResponse = serde_json::from_value(json!({
            "choices": [{"message": {"content": " Summary. ", "reasoning": "private"}}]
        }))
        .unwrap();
        assert_eq!(response.choices[0].message.visible_text(), "Summary.");
    }

    #[test]
    fn reasoning_effort_rejection_matcher() {
        let http = |status, body: &str| LlmError::Http {
            status,
            body: body.to_string(),
        };
        assert!(ollama_rejected_reasoning_effort(&http(
            400,
            r#"{"error":{"param":"reasoning_effort"}}"#
        )));
        assert!(ollama_rejected_reasoning_effort(&http(
            422,
            "model does not support Thinking"
        )));
        assert!(!ollama_rejected_reasoning_effort(&LlmError::AuthFailed {
            status: 401,
            body: "reasoning_effort".to_string(),
        }));
        assert!(!ollama_rejected_reasoning_effort(&http(
            500,
            "reasoning_effort"
        )));
        assert!(!ollama_rejected_reasoning_effort(&http(
            400,
            r#"{"error":"invalid model"}"#
        )));
    }

    const REASONING_400: &str = r#"{"error":{"param":"reasoning_effort"}}"#;

    fn ok_body(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"content": text}}]})
    }

    async fn call(provider: LLMProvider, server: &MockServer) -> Result<String, String> {
        let ollama = server.uri();
        let custom = format!("{}/v1", server.uri());
        generate_summary(
            crate::llm::shared_client(),
            &provider,
            "m",
            "",
            "sys",
            "user",
            Some(&ollama),
            Some(&custom),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
    }

    async fn request_bodies(server: &MockServer) -> Vec<serde_json::Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn ollama_rejecting_reasoning_effort_is_resent_once_without_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string(REASONING_400))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body("Done.")))
            .mount(&server)
            .await;

        assert_eq!(call(LLMProvider::Ollama, &server).await.unwrap(), "Done.");

        let bodies = request_bodies(&server).await;
        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["reasoning_effort"], "none");
        assert!(bodies[1].get("reasoning_effort").is_none());
        assert_eq!(bodies[0]["messages"], bodies[1]["messages"]);
    }

    #[tokio::test]
    async fn ollama_compatibility_resend_is_bounded_to_one() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string(REASONING_400))
            .mount(&server)
            .await;

        let err = call(LLMProvider::Ollama, &server).await.unwrap_err();
        assert!(err.contains("reasoning_effort"), "{err}");
        assert_eq!(request_bodies(&server).await.len(), 2);
    }

    #[tokio::test]
    async fn ollama_unrelated_400_is_not_resent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(400).set_body_string(r#"{"error":"invalid model"}"#),
            )
            .mount(&server)
            .await;

        assert!(call(LLMProvider::Ollama, &server).await.is_err());
        assert_eq!(request_bodies(&server).await.len(), 1);
    }

    #[tokio::test]
    async fn non_ollama_reasoning_400_is_not_resent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string(REASONING_400))
            .mount(&server)
            .await;

        assert!(call(LLMProvider::CustomOpenAI, &server).await.is_err());
        let bodies = request_bodies(&server).await;
        assert_eq!(bodies.len(), 1);
        assert!(bodies[0].get("reasoning_effort").is_none());
    }
}
