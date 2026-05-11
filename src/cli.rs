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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Workflow {
    Summarize,
    Analyze,
    Plan,
    Ideas,
    Report,
}

impl Workflow {
    pub fn as_str(self) -> &'static str {
        match self {
            Workflow::Summarize => "summarize",
            Workflow::Analyze => "analyze",
            Workflow::Plan => "plan",
            Workflow::Ideas => "ideas",
            Workflow::Report => "report",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Json,
    Both,
}
