use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "deepcode")]
#[command(about = "DeepSeek-powered read-only code analysis")]
pub struct Cli {
    /// Config file path. Defaults to searching for .deepcode.toml from the current directory upward.
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// Override the configured DeepSeek-compatible base URL.
    #[arg(long, global = true)]
    pub base_url: Option<String>,
    /// Override the configured model.
    #[arg(long, global = true)]
    pub model: Option<String>,
    /// Override the maximum output tokens requested from DeepSeek.
    #[arg(long, global = true)]
    pub max_tokens: Option<u32>,
    /// Override whether DeepSeek thinking mode is enabled.
    #[arg(long, global = true)]
    pub thinking_enabled: Option<bool>,
    /// Override DeepSeek reasoning effort when thinking mode is enabled.
    #[arg(long, global = true)]
    pub reasoning_effort: Option<String>,
    /// Override how many times DeepCode attempts each DeepSeek request.
    #[arg(long, global = true)]
    pub retry_attempts: Option<usize>,
    /// Override retry backoff in milliseconds.
    #[arg(long, global = true)]
    pub retry_backoff_ms: Option<u64>,
    /// Override the total DeepSeek HTTP request timeout in seconds.
    #[arg(long, global = true)]
    pub api_timeout_secs: Option<u64>,
    /// Override the configured output directory.
    #[arg(long, global = true)]
    pub output_dir: Option<PathBuf>,
    /// Override the configured output format.
    #[arg(long, global = true, value_enum)]
    pub format: Option<OutputFormat>,
    /// Override the per-file byte limit sent to the model.
    #[arg(long, global = true)]
    pub max_file_bytes: Option<u64>,
    /// Override the maximum number of files sent to the model.
    #[arg(long, global = true)]
    pub max_files: Option<usize>,
    /// Override the total byte budget sent to the model.
    #[arg(long, global = true)]
    pub max_total_bytes: Option<u64>,
    /// Override the maximum number of files read concurrently.
    #[arg(long, global = true)]
    pub max_concurrency: Option<usize>,
    /// Disable reading and writing cached model responses for this run.
    #[arg(long, global = true)]
    pub no_cache: bool,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Explain file or project responsibilities and risks.
    Summarize(PathCommand),
    /// Produce a quality and maintainability report.
    Analyze(PathCommand),
    /// Generate an implementation plan for a goal.
    Plan(PlanCommand),
    /// Suggest features, optimizations, and architecture ideas.
    Ideas(PathCommand),
    /// Generate a combined Markdown and JSON report.
    Report(PathCommand),
    /// Build a structured code understanding map.
    Understand(PathCommand),
    /// Generate practical project documentation.
    Docs(DocsCommand),
    /// Generate a prioritized refactoring plan.
    Refactor(PlanCommand),
    /// Compare two paths and report structural and behavioral differences.
    Diff(DiffCommand),
    /// Start an interactive exploration session.
    Explore(PathCommand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workflow {
    Summarize,
    Analyze,
    Plan,
    Ideas,
    Report,
    Understand,
    Docs,
    Refactor,
    Diff,
    Explore,
}

impl Workflow {
    pub fn as_str(self) -> &'static str {
        match self {
            Workflow::Summarize => "summarize",
            Workflow::Analyze => "analyze",
            Workflow::Plan => "plan",
            Workflow::Ideas => "ideas",
            Workflow::Report => "report",
            Workflow::Understand => "understand",
            Workflow::Docs => "docs",
            Workflow::Refactor => "refactor",
            Workflow::Diff => "diff",
            Workflow::Explore => "explore",
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct PathCommand {
    /// File or project directory to read.
    pub path: PathBuf,
}

#[derive(Debug, clap::Args)]
pub struct PlanCommand {
    /// File or project directory to read.
    pub path: PathBuf,
    /// Development goal to plan for.
    #[arg(long)]
    pub goal: String,
}

#[derive(Debug, clap::Args)]
pub struct DocsCommand {
    /// File or project directory to read.
    pub path: PathBuf,
    /// Documentation kind to focus on. Repeat for multiple kinds.
    #[arg(long = "kind", value_enum)]
    pub kinds: Vec<DocKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DocKind {
    Readme,
    Architecture,
    Api,
    Onboarding,
    Changelog,
}

impl DocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DocKind::Readme => "readme",
            DocKind::Architecture => "architecture",
            DocKind::Api => "api",
            DocKind::Onboarding => "onboarding",
            DocKind::Changelog => "changelog",
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct DiffCommand {
    /// Baseline file or project directory.
    pub old_path: PathBuf,
    /// Changed file or project directory.
    pub new_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Json,
    Both,
}
