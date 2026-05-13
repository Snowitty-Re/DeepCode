use crate::cli::Workflow;
use crate::config::Config;
use crate::scanner::ProjectSnapshot;
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

const CACHE_INPUT_VERSION: u8 = 2;

#[derive(Debug, Serialize)]
struct CacheInput<'a> {
    version: u8,
    workflow: Workflow,
    goal: Option<&'a str>,
    model: &'a str,
    base_url: &'a str,
    max_tokens: u32,
    thinking_enabled: bool,
    reasoning_effort: &'a str,
    api_timeout_secs: u64,
    snapshot: &'a ProjectSnapshot,
}

pub fn cache_key(
    workflow: Workflow,
    goal: Option<&str>,
    config: &Config,
    snapshot: &ProjectSnapshot,
) -> Result<String> {
    let input = CacheInput {
        version: CACHE_INPUT_VERSION,
        workflow,
        goal,
        model: &config.model,
        base_url: &config.base_url,
        max_tokens: config.max_tokens,
        thinking_enabled: config.thinking_enabled,
        reasoning_effort: &config.reasoning_effort,
        api_timeout_secs: config.api_timeout_secs,
        snapshot,
    };
    let bytes = serde_json::to_vec(&input).context("failed to serialize cache key input")?;
    Ok(hex_digest(&bytes))
}

pub fn read_cached(config: &Config, key: &str) -> Result<Option<String>> {
    let path = cache_path(config, key);
    if !path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read cache entry {}", path.display()))?;
    Ok(Some(content))
}

pub fn write_cached(config: &Config, key: &str, content: &str) -> Result<()> {
    let path = cache_path(config, key);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache directory {}", parent.display()))?;
    }
    fs::write(&path, content)
        .with_context(|| format!("failed to write cache entry {}", path.display()))?;
    Ok(())
}

fn cache_path(config: &Config, key: &str) -> PathBuf {
    config.output_dir.join(".cache").join(format!("{key}.json"))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReportFormat;
    use crate::scanner::ScannedFile;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn cache_key_changes_when_content_changes() {
        let config = test_config();
        let first = snapshot("fn main() {}");
        let second = snapshot("fn main() { println!(\"hi\"); }");

        let first_key = cache_key(Workflow::Summarize, None, &config, &first).unwrap();
        let second_key = cache_key(Workflow::Summarize, None, &config, &second).unwrap();

        assert_ne!(first_key, second_key);
        assert_eq!(first_key.len(), 64);
    }

    #[test]
    fn reads_and_writes_cached_content() {
        let config = test_config();

        write_cached(&config, "abc", "{\"summary\":\"ok\"}").unwrap();
        let cached = read_cached(&config, "abc").unwrap();

        assert_eq!(cached.as_deref(), Some("{\"summary\":\"ok\"}"));
    }

    fn snapshot(content: &str) -> ProjectSnapshot {
        ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files: vec![ScannedFile {
                path: PathBuf::from("src/main.rs"),
                language: "Rust".to_string(),
                bytes: content.len() as u64,
                truncated: false,
                metrics: crate::scanner::FileMetrics {
                    lines: 1,
                    code_lines: 1,
                    comment_lines: 0,
                    blank_lines: 0,
                    longest_line: content.len(),
                },
                content: content.to_string(),
            }],
            skipped: vec![],
            summary: crate::scanner::ScanSummary {
                files_read: 1,
                files_skipped: 0,
                bytes_read: content.len() as u64,
                total_lines: 1,
                total_code_lines: 1,
                languages: vec![crate::scanner::LanguageSummary {
                    language: "Rust".to_string(),
                    files: 1,
                    bytes: content.len() as u64,
                    code_lines: 1,
                }],
            },
        }
    }

    fn test_config() -> Config {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Config {
            api_key: "sk-test".to_string(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            max_tokens: 16_384,
            thinking_enabled: false,
            reasoning_effort: "high".to_string(),
            retry_attempts: 3,
            retry_backoff_ms: 1_000,
            api_timeout_secs: 600,
            output_dir: std::env::temp_dir().join(format!("deepcode-cache-{unique}")),
            format: ReportFormat::Both,
            max_file_bytes: 200_000,
            max_files: 200,
            max_total_bytes: 2_000_000,
            max_concurrency: 4,
            cache_enabled: true,
        }
    }
}
