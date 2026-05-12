use crate::cli::OutputFormat;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = ".deepcode.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_tokens: u32,
    pub thinking_enabled: bool,
    pub reasoning_effort: String,
    pub retry_attempts: usize,
    pub retry_backoff_ms: u64,
    pub api_timeout_secs: u64,
    pub output_dir: PathBuf,
    pub format: ReportFormat,
    pub max_file_bytes: u64,
    pub max_files: usize,
    pub max_total_bytes: u64,
    pub max_concurrency: usize,
    pub cache_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportFormat {
    Markdown,
    Json,
    Both,
}

impl ReportFormat {
    pub fn writes_markdown(self) -> bool {
        matches!(self, Self::Markdown | Self::Both)
    }

    pub fn writes_json(self) -> bool {
        matches!(self, Self::Json | Self::Both)
    }
}

impl From<OutputFormat> for ReportFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Markdown => Self::Markdown,
            OutputFormat::Json => Self::Json,
            OutputFormat::Both => Self::Both,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    max_tokens: Option<u32>,
    thinking_enabled: Option<bool>,
    reasoning_effort: Option<String>,
    retry_attempts: Option<usize>,
    retry_backoff_ms: Option<u64>,
    api_timeout_secs: Option<u64>,
    output_dir: Option<PathBuf>,
    format: Option<ReportFormat>,
    max_file_bytes: Option<u64>,
    max_files: Option<usize>,
    max_total_bytes: Option<u64>,
    max_concurrency: Option<usize>,
    cache_enabled: Option<bool>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            max_tokens: 16_384,
            thinking_enabled: false,
            reasoning_effort: "high".to_string(),
            retry_attempts: 3,
            retry_backoff_ms: 1_000,
            api_timeout_secs: 600,
            output_dir: PathBuf::from("deepcode-reports"),
            format: ReportFormat::Both,
            max_file_bytes: 200_000,
            max_files: 200,
            max_total_bytes: 2_000_000,
            max_concurrency: 4,
            cache_enabled: true,
        }
    }
}

impl Config {
    pub fn load(start_dir: impl AsRef<Path>) -> Result<Self> {
        let config_path = find_config(start_dir.as_ref()).with_context(|| {
            format!(
                "could not find {CONFIG_FILE}; copy .deepcode.example.toml to {CONFIG_FILE} and fill in api_key"
            )
        })?;
        Self::load_file(config_path)
    }

    pub fn validate_public(&self) -> Result<()> {
        self.validate()
    }

    pub fn load_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        let mut config = Config::default();
        raw.apply_to(&mut config);
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.api_key.trim().is_empty() || self.api_key == "sk-your-deepseek-api-key" {
            bail!("missing DeepSeek api_key in {CONFIG_FILE}; copy .deepcode.example.toml to {CONFIG_FILE} and fill it in manually");
        }
        if self.base_url.trim().is_empty() {
            bail!("base_url cannot be empty");
        }
        if self.model.trim().is_empty() {
            bail!("model cannot be empty");
        }
        if self.max_tokens == 0 {
            bail!("max_tokens must be greater than zero");
        }
        if self.thinking_enabled && !matches!(self.reasoning_effort.as_str(), "high" | "max") {
            bail!("reasoning_effort must be high or max when thinking_enabled is true");
        }
        if self.retry_attempts == 0 {
            bail!("retry_attempts must be greater than zero");
        }
        if self.api_timeout_secs == 0 {
            bail!("api_timeout_secs must be greater than zero");
        }
        if self.max_file_bytes == 0 {
            bail!("max_file_bytes must be greater than zero");
        }
        if self.max_files == 0 {
            bail!("max_files must be greater than zero");
        }
        if self.max_total_bytes == 0 {
            bail!("max_total_bytes must be greater than zero");
        }
        if self.max_concurrency == 0 {
            bail!("max_concurrency must be greater than zero");
        }
        Ok(())
    }
}

impl RawConfig {
    fn apply_to(self, config: &mut Config) {
        apply(self.api_key, &mut config.api_key);
        apply(self.base_url, &mut config.base_url);
        apply(self.model, &mut config.model);
        apply(self.max_tokens, &mut config.max_tokens);
        apply(self.thinking_enabled, &mut config.thinking_enabled);
        apply(self.reasoning_effort, &mut config.reasoning_effort);
        apply(self.retry_attempts, &mut config.retry_attempts);
        apply(self.retry_backoff_ms, &mut config.retry_backoff_ms);
        apply(self.api_timeout_secs, &mut config.api_timeout_secs);
        apply(self.output_dir, &mut config.output_dir);
        apply(self.format, &mut config.format);
        apply(self.max_file_bytes, &mut config.max_file_bytes);
        apply(self.max_files, &mut config.max_files);
        apply(self.max_total_bytes, &mut config.max_total_bytes);
        apply(self.max_concurrency, &mut config.max_concurrency);
        apply(self.cache_enabled, &mut config.cache_enabled);
    }
}

fn apply<T>(value: Option<T>, target: &mut T) {
    if let Some(value) = value {
        *target = value;
    }
}

fn find_config(start_dir: &Path) -> Option<PathBuf> {
    let mut current = if start_dir.is_file() {
        start_dir.parent()?.to_path_buf()
    } else {
        start_dir.to_path_buf()
    };

    loop {
        let candidate = current.join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn applies_defaults_for_optional_fields() {
        let dir = temp_dir("defaults");
        let path = dir.join(CONFIG_FILE);
        fs::write(&path, "api_key = \"sk-test\"\n").unwrap();

        let config = Config::load_file(&path).unwrap();

        assert_eq!(config.api_key, "sk-test");
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.model, "deepseek-v4-pro");
        assert_eq!(config.max_tokens, 16_384);
        assert!(!config.thinking_enabled);
        assert_eq!(config.reasoning_effort, "high");
        assert_eq!(config.retry_attempts, 3);
        assert_eq!(config.retry_backoff_ms, 1_000);
        assert_eq!(config.api_timeout_secs, 600);
        assert_eq!(config.output_dir, PathBuf::from("deepcode-reports"));
        assert_eq!(config.format, ReportFormat::Both);
        assert_eq!(config.max_file_bytes, 200_000);
        assert_eq!(config.max_files, 200);
        assert_eq!(config.max_total_bytes, 2_000_000);
        assert_eq!(config.max_concurrency, 4);
        assert!(config.cache_enabled);
    }

    #[test]
    fn rejects_missing_api_key() {
        let dir = temp_dir("missing-key");
        let path = dir.join(CONFIG_FILE);
        fs::write(&path, "model = \"deepseek-v4-pro\"\n").unwrap();

        let error = Config::load_file(&path).unwrap_err().to_string();

        assert!(error.contains("missing DeepSeek api_key"));
    }

    #[test]
    fn finds_config_from_child_directory() {
        let dir = temp_dir("find");
        let child = dir.join("a").join("b");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.join(CONFIG_FILE), "api_key = \"sk-test\"\n").unwrap();

        let config = Config::load(&child).unwrap();

        assert_eq!(config.api_key, "sk-test");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("deepcode-{name}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
