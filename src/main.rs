use anyhow::Result;
use clap::Parser;
use deepcode::cli::{Cli, Commands, Workflow};
use deepcode::config::Config;
use deepcode::deepseek::DeepSeekClient;
use deepcode::prompts::build_messages;
use deepcode::report::{parse_report, write_report, WrittenReport};
use deepcode::scanner::{scan_path, ScanOptions};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(std::env::current_dir()?)?;
    let client = DeepSeekClient::new(&config)?;

    match cli.command {
        Commands::Summarize(cmd) => {
            run_workflow(Workflow::Summarize, cmd.path, None, &config, &client)
        }
        Commands::Analyze(cmd) => run_workflow(Workflow::Analyze, cmd.path, None, &config, &client),
        Commands::Plan(cmd) => {
            run_workflow(Workflow::Plan, cmd.path, Some(cmd.goal), &config, &client)
        }
        Commands::Ideas(cmd) => run_workflow(Workflow::Ideas, cmd.path, None, &config, &client),
        Commands::Report(cmd) => run_workflow(Workflow::Report, cmd.path, None, &config, &client),
    }
}

fn run_workflow(
    workflow: Workflow,
    path: std::path::PathBuf,
    goal: Option<String>,
    config: &Config,
    client: &DeepSeekClient,
) -> Result<()> {
    let snapshot = scan_path(
        &path,
        ScanOptions {
            max_file_bytes: config.max_file_bytes,
        },
    )?;
    let messages = build_messages(workflow, &snapshot, goal.as_deref())?;
    let raw = client.complete(&messages, true)?;
    let report = parse_report(&raw)?;
    let written = write_report(config, workflow, &snapshot, &report)?;
    print_written_report(written);
    Ok(())
}

fn print_written_report(written: WrittenReport) {
    if let Some(path) = written.markdown_path {
        println!("Markdown report: {}", path.display());
    }
    if let Some(path) = written.json_path {
        println!("JSON report: {}", path.display());
    }
}
