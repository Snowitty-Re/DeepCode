use crate::config::Config;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    http: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    thinking_enabled: bool,
    reasoning_effort: String,
    retry_attempts: usize,
    retry_backoff_ms: u64,
    api_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
    thinking: Thinking,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Serialize)]
struct Thinking {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: AssistantMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: Option<String>,
}

impl DeepSeekClient {
    pub fn new(config: &Config) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_secs))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            thinking_enabled: config.thinking_enabled,
            reasoning_effort: config.reasoning_effort.clone(),
            retry_attempts: config.retry_attempts,
            retry_backoff_ms: config.retry_backoff_ms,
            api_timeout_secs: config.api_timeout_secs,
        })
    }

    pub fn complete(&self, messages: &[ChatMessage], json_mode: bool) -> Result<String> {
        self.complete_with_progress(messages, json_mode, |_| {})
    }

    pub fn complete_with_progress(
        &self,
        messages: &[ChatMessage],
        json_mode: bool,
        progress: impl Fn(&str),
    ) -> Result<String> {
        let request = build_request(
            &self.model,
            messages,
            json_mode,
            self.max_tokens,
            self.thinking_enabled,
            &self.reasoning_effort,
        );
        let url = format!("{}/chat/completions", self.base_url);
        for attempt in 1..=self.retry_attempts {
            progress(&format!(
                "Sending DeepSeek request to {} with model {} (attempt {}/{}, timeout {}s)",
                self.base_url, self.model, attempt, self.retry_attempts, self.api_timeout_secs
            ));
            let response = match self
                .http
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&request)
                .send()
            {
                Ok(response) => response,
                Err(error) if attempt < self.retry_attempts => {
                    progress(&format!("Request failed: {error}; retrying"));
                    self.sleep_before_retry(attempt);
                    continue;
                }
                Err(error) => {
                    return Err(anyhow!(
                        "DeepSeek request failed after {} attempt(s): {error}. Check base_url, network access, and proxy settings.",
                        self.retry_attempts
                    ));
                }
            };

            let status = response.status();
            let body = match response.text() {
                Ok(body) => body,
                Err(error) if attempt < self.retry_attempts => {
                    progress(&format!(
                        "Failed to read DeepSeek response: {error}; retrying"
                    ));
                    self.sleep_before_retry(attempt);
                    continue;
                }
                Err(error) => {
                    return Err(anyhow!(
                        "Failed to read DeepSeek response after {} attempt(s): {error}. The model may still be generating; increase api_timeout_secs, reduce scanned input, or try deepseek-v4-flash.",
                        self.retry_attempts
                    ));
                }
            };
            match parse_response(status, &body) {
                Ok(content) => return Ok(content),
                Err(error) if attempt < self.retry_attempts && should_retry(status, &error) => {
                    progress(&format!("{error}; retrying"));
                    self.sleep_before_retry(attempt);
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("retry loop either returns content or an error")
    }

    fn sleep_before_retry(&self, attempt: usize) {
        let delay = self.retry_backoff_ms.saturating_mul(attempt as u64);
        thread::sleep(Duration::from_millis(delay));
    }
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }
}

fn build_request<'a>(
    model: &'a str,
    messages: &'a [ChatMessage],
    json_mode: bool,
    max_tokens: u32,
    thinking_enabled: bool,
    reasoning_effort: &'a str,
) -> ChatCompletionRequest<'a> {
    ChatCompletionRequest {
        model,
        messages,
        stream: false,
        max_tokens,
        temperature: (!thinking_enabled).then_some(0.2),
        response_format: json_mode.then_some(ResponseFormat {
            kind: "json_object",
        }),
        thinking: Thinking {
            kind: if thinking_enabled {
                "enabled"
            } else {
                "disabled"
            },
        },
        reasoning_effort: thinking_enabled.then_some(reasoning_effort),
    }
}

fn parse_response(status: StatusCode, body: &str) -> Result<String> {
    if !status.is_success() {
        bail!(
            "DeepSeek API returned {status}: {} {}",
            api_error_detail(body),
            status_hint(status)
        );
    }

    let response: ChatCompletionResponse =
        serde_json::from_str(body).context("failed to parse DeepSeek chat completion response")?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .context("DeepSeek response did not include choices")?;
    if let Some(finish_reason) = choice.finish_reason.as_deref() {
        match finish_reason {
            "length" => bail!(
                "DeepSeek response was truncated because max_tokens or context length was reached; increase max_tokens or reduce scanned input"
            ),
            "content_filter" => bail!("DeepSeek response was blocked by the content filter"),
            "insufficient_system_resource" => {
                bail!("DeepSeek response stopped because backend inference resources were insufficient")
            }
            _ => {}
        }
    }
    let content = choice
        .message
        .content
        .filter(|content| !content.trim().is_empty())
        .context("DeepSeek response did not include message content")?;
    Ok(content)
}

fn should_retry(status: StatusCode, error: &anyhow::Error) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    ) || (status.is_success()
        && error
            .to_string()
            .contains("DeepSeek response did not include message content"))
        || (status.is_success()
            && error
                .to_string()
                .contains("backend inference resources were insufficient"))
}

fn api_error_detail(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return body.to_string();
    };
    if let Some(message) = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return message.to_string();
    }
    value.to_string()
}

fn status_hint(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "Request body format is invalid; check model parameters and JSON mode settings.",
        401 => "Authentication failed; check api_key in .deepcode.toml.",
        402 => "Account balance is insufficient; check your DeepSeek account balance.",
        422 => "Request parameters are invalid; check model name, max_tokens, thinking, and response_format.",
        429 => "Rate limit reached; DeepCode retried this request when configured to do so.",
        500 => "DeepSeek server error; retry later if it persists.",
        503 => "DeepSeek service is busy; retry later if it persists.",
        _ => "Check DeepSeek API status, base_url, and request parameters.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_json_mode_request() {
        let messages = vec![
            ChatMessage::system("You are concise"),
            ChatMessage::user("Hi"),
        ];
        let request = build_request("deepseek-v4-pro", &messages, true, 16_384, false, "high");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "deepseek-v4-pro");
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["response_format"]["type"], "json_object");
        assert_eq!(value["stream"], false);
        assert_eq!(value["max_tokens"], 16_384);
        assert_eq!(value["thinking"]["type"], "disabled");
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn serializes_thinking_request_without_temperature() {
        let messages = vec![ChatMessage::user("Hi")];
        let request = build_request("deepseek-v4-pro", &messages, false, 4096, true, "max");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["reasoning_effort"], "max");
        assert!(value.get("temperature").is_none());
    }

    #[test]
    fn parses_chat_completion_content() {
        let body = r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#;

        let content = parse_response(StatusCode::OK, body).unwrap();

        assert_eq!(content, "{\"ok\":true}");
    }

    #[test]
    fn treats_empty_message_content_as_retryable_parse_error() {
        let body = r#"{"choices":[{"message":{"content":""}}]}"#;

        let error = parse_response(StatusCode::OK, body).unwrap_err();

        assert!(should_retry(StatusCode::OK, &error));
    }

    #[test]
    fn reports_truncated_finish_reason() {
        let body = r#"{"choices":[{"finish_reason":"length","message":{"content":"{\"ok\""}}]}"#;

        let error = parse_response(StatusCode::OK, body)
            .unwrap_err()
            .to_string();

        assert!(error.contains("truncated"));
        assert!(error.contains("max_tokens"));
    }

    #[test]
    fn retries_insufficient_backend_resources() {
        let body = r#"{"choices":[{"finish_reason":"insufficient_system_resource","message":{"content":""}}]}"#;

        let error = parse_response(StatusCode::OK, body).unwrap_err();

        assert!(should_retry(StatusCode::OK, &error));
    }

    #[test]
    fn maps_error_status_to_error() {
        let error = parse_response(StatusCode::UNAUTHORIZED, "{\"error\":\"bad key\"}")
            .unwrap_err()
            .to_string();

        assert!(error.contains("401 Unauthorized"));
        assert!(error.contains("bad key"));
    }
}
