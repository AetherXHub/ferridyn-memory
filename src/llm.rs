//! LLM client abstraction and Anthropic API implementation.
//!
//! This module provides a generic [`LlmClient`] trait for interacting with
//! language models, along with concrete implementations:
//!
//! - [`AnthropicClient`]: production client for Anthropic's Claude API
//! - [`MockLlmClient`]: test double for unit tests
//!
//! Used by the schema system for inference and natural language recall resolution.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during LLM operations.
#[derive(Debug, Error)]
pub enum LlmError {
    /// The ANTHROPIC_API_KEY environment variable is not set.
    #[error("ANTHROPIC_API_KEY environment variable not set")]
    MissingApiKey,

    /// HTTP or network error occurred.
    #[error("HTTP error: {0}")]
    Http(String),

    /// Failed to parse the API response.
    #[error("Parse error: {0}")]
    Parse(String),

    /// Model returned no text content.
    #[error("Model returned empty response")]
    EmptyResponse,
}

// ============================================================================
// Completion Type
// ============================================================================

/// The result of a successful LLM completion request.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The generated text from the model.
    pub text: String,
}

// ============================================================================
// LlmClient Trait
// ============================================================================

/// Generic interface for LLM clients.
///
/// Supports simple system+user prompt completion with text response.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Generate a completion given a system prompt and user message.
    ///
    /// # Arguments
    ///
    /// * `system` - System-level instructions for the model
    /// * `user` - User message or prompt
    ///
    /// # Returns
    ///
    /// A [`Completion`] containing the model's response text.
    async fn complete(&self, system: &str, user: &str) -> Result<Completion, LlmError>;
}

// ============================================================================
// Anthropic API Implementation
// ============================================================================

/// Client for the Anthropic Claude API.
///
/// Makes HTTP requests to the Anthropic Messages API endpoint to generate
/// completions using Claude models.
pub struct AnthropicClient {
    api_key: String,
    model: String,
    max_tokens: u32,
    client: reqwest::Client,
}

/// Request body for the Anthropic Messages API.
#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
}

/// A message in the conversation.
#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

/// Response from the Anthropic Messages API.
#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

/// A content block in the API response.
#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
}

impl AnthropicClient {
    /// Create a new client by reading the API key from the environment.
    ///
    /// Reads the `ANTHROPIC_API_KEY` environment variable. Uses default
    /// model `claude-haiku-4-5` and max tokens `2048`.
    ///
    /// # Errors
    ///
    /// Returns [`LlmError::MissingApiKey`] if the environment variable is not set.
    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        Ok(Self::new(api_key))
    }

    /// Create a new client with an explicit API key.
    ///
    /// Uses default model `claude-haiku-4-5` and max tokens `2048`.
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "claude-haiku-4-5".to_string(),
            max_tokens: 2048,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, system: &str, user: &str) -> Result<Completion, LlmError> {
        let request_body = AnthropicRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system: system.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: user.to_string(),
            }],
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        let api_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;

        let text = api_response
            .content
            .into_iter()
            .next()
            .ok_or(LlmError::EmptyResponse)?
            .text;

        Ok(Completion { text })
    }
}

// ============================================================================
// Mock Implementation (Test Only)
// ============================================================================

/// Mock LLM client for testing. Returns pre-programmed responses in FIFO order.
#[cfg(test)]
pub struct MockLlmClient {
    /// Pre-programmed responses to return in FIFO order.
    pub responses: std::sync::Mutex<std::collections::VecDeque<String>>,
}

#[cfg(test)]
impl MockLlmClient {
    /// Create a new mock client with a sequence of responses.
    ///
    /// Each call to [`complete`](LlmClient::complete) will return the next
    /// response in order.
    ///
    /// # Panics
    ///
    /// Panics if [`complete`](LlmClient::complete) is called more times
    /// than there are responses.
    pub fn new(responses: Vec<String>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses.into()),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, _system: &str, _user: &str) -> Result<Completion, LlmError> {
        let text = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockLlmClient: no more responses available");

        Ok(Completion { text })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_env_missing_key() {
        // Ensure the environment variable is not set.
        // SAFETY: This test runs serially and no other thread reads ANTHROPIC_API_KEY concurrently.
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

        let result = AnthropicClient::from_env();
        assert!(matches!(result, Err(LlmError::MissingApiKey)));
    }

    #[tokio::test]
    async fn test_mock_returns_responses_in_order() {
        let mock = MockLlmClient::new(vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ]);

        let completion1 = mock.complete("sys", "user").await.unwrap();
        assert_eq!(completion1.text, "first");

        let completion2 = mock.complete("sys", "user").await.unwrap();
        assert_eq!(completion2.text, "second");

        let completion3 = mock.complete("sys", "user").await.unwrap();
        assert_eq!(completion3.text, "third");
    }

    #[tokio::test]
    async fn test_mock_completion_text() {
        let mock = MockLlmClient::new(vec!["Hello, world!".to_string()]);

        let completion = mock
            .complete("You are a helpful assistant.", "Say hello.")
            .await
            .unwrap();

        assert_eq!(completion.text, "Hello, world!");
    }
}
