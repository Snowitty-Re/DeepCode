use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "deepcode")]
#[command(about = "DeepSeek-powered read-only code analysis")]
pub struct Cli {
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
