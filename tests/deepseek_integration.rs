use deepcode::config::{Config, ReportFormat};
use deepcode::deepseek::{ChatMessage, DeepSeekClient};
use httpmock::prelude::*;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn sends_openai_compatible_chat_completion_request() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/chat/completions")
            .header("authorization", "Bearer sk-test")
            .json_body_partial(r#"{"model":"deepseek-v4-pro"}"#)
            .json_body_partial(r#"{"max_tokens":16384}"#)
            .json_body_partial(r#"{"stream":false}"#)
            .json_body_partial(r#"{"thinking":{"type":"disabled"}}"#)
            .json_body_partial(r#"{"response_format":{"type":"json_object"}}"#);
        then.status(200).json_body(json!({
            "choices": [
                {
                    "message": {
                        "content": "{\"summary\":\"ok\",\"quality\":{\"score\":90}}"
                    }
                }
            ]
        }));
    });
    let config = Config {
        api_key: "sk-test".to_string(),
        base_url: server.base_url(),
        model: "deepseek-v4-pro".to_string(),
        max_tokens: 16_384,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        retry_attempts: 3,
        retry_backoff_ms: 1_000,
        api_timeout_secs: 600,
        output_dir: PathBuf::from("target/test-reports"),
        format: ReportFormat::Both,
        max_file_bytes: 200_000,
        max_files: 200,
        max_total_bytes: 2_000_000,
        max_concurrency: 4,
        cache_enabled: false,
    };
    let client = DeepSeekClient::new(&config).unwrap();

    let content = client
        .complete(
            &[
                ChatMessage::system("Return JSON"),
                ChatMessage::user("Analyze this"),
            ],
            true,
        )
        .unwrap();

    mock.assert();
    assert!(content.contains("\"summary\":\"ok\""));
}
