use crate::config::Config;
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct DeepSeekClient {
    http: Client,
    api_key: String,
    base_url: String,
    model: String,
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
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
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
}

#[derive(Debug, Deserialize)]
struct AssistantMessage {
    content: String,
}

impl DeepSeekClient {
    pub fn new(config: &Config) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self {
            http,
            api_key: config.api_key.clone(),
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
        })
    }

    pub fn complete(&self, messages: &[ChatMessage], json_mode: bool) -> Result<String> {
        let request = build_request(&self.model, messages, json_mode);
        let url = format!("{}/chat/completions", self.base_url);
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .context("DeepSeek request failed")?;

        let status = response.status();
        let body = response
            .text()
            .context("failed to read DeepSeek response")?;
        parse_response(status, &body)
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
) -> ChatCompletionRequest<'a> {
    ChatCompletionRequest {
        model,
        messages,
        temperature: 0.2,
        response_format: json_mode.then_some(ResponseFormat {
            kind: "json_object",
        }),
    }
}

fn parse_response(status: StatusCode, body: &str) -> Result<String> {
    if !status.is_success() {
        bail!("DeepSeek API returned {status}: {body}");
    }

    let response: ChatCompletionResponse =
        serde_json::from_str(body).context("failed to parse DeepSeek chat completion response")?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .context("DeepSeek response did not include message content")?;
    Ok(content)
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
        let request = build_request("deepseek-v4-pro", &messages, true);
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["model"], "deepseek-v4-pro");
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["response_format"]["type"], "json_object");
    }

    #[test]
    fn parses_chat_completion_content() {
        let body = r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#;

        let content = parse_response(StatusCode::OK, body).unwrap();

        assert_eq!(content, "{\"ok\":true}");
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
