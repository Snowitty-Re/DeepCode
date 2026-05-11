use anyhow::Result;
use clap::Parser;
use deepcode::cache::{cache_key, read_cached, write_cached};
use deepcode::cli::{Cli, Commands, Workflow};
use deepcode::config::Config;
use deepcode::deepseek::DeepSeekClient;
use deepcode::prompts::build_messages;
use deepcode::report::{parse_report, write_report, WrittenReport};
use deepcode::scanner::{scan_path, ProjectSnapshot, ScanOptions};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(std::env::current_dir()?)?;

    match cli.command {
        Commands::Summarize(cmd) => {
            run_workflow(Workflow::Summarize, cmd.path, None, &config, cli.no_cache)
        }
        Commands::Analyze(cmd) => {
            run_workflow(Workflow::Analyze, cmd.path, None, &config, cli.no_cache)
        }
        Commands::Plan(cmd) => run_workflow(
            Workflow::Plan,
            cmd.path,
            Some(cmd.goal),
            &config,
            cli.no_cache,
        ),
        Commands::Ideas(cmd) => {
            run_workflow(Workflow::Ideas, cmd.path, None, &config, cli.no_cache)
        }
        Commands::Report(cmd) => {
            run_workflow(Workflow::Report, cmd.path, None, &config, cli.no_cache)
        }
    }
}

fn run_workflow(
    workflow: Workflow,
    path: std::path::PathBuf,
    goal: Option<String>,
    config: &Config,
    no_cache: bool,
) -> Result<()> {
    let snapshot = scan_path(
        &path,
        ScanOptions {
            max_file_bytes: config.max_file_bytes,
        },
    )?;
    let cache_enabled = config.cache_enabled && !no_cache;
    let key = cache_key(workflow, goal.as_deref(), config, &snapshot)?;
    let raw = if cache_enabled {
        match read_cached(config, &key)? {
            Some(content) => {
                println!("Using cached DeepSeek response");
                content
            }
            None => {
                let content = request_model(workflow, goal.as_deref(), config, &snapshot)?;
                write_cached(config, &key, &content)?;
                content
            }
        }
    } else {
        request_model(workflow, goal.as_deref(), config, &snapshot)?
    };
    let report = parse_report(&raw)?;
    let written = write_report(config, workflow, &snapshot, &report)?;
    print_written_report(written);
    Ok(())
}

fn request_model(
    workflow: Workflow,
    goal: Option<&str>,
    config: &Config,
    snapshot: &ProjectSnapshot,
) -> Result<String> {
    let client = DeepSeekClient::new(config)?;
    let messages = build_messages(workflow, snapshot, goal)?;
    client.complete(&messages, true)
}

fn print_written_report(written: WrittenReport) {
    if let Some(path) = written.markdown_path {
        println!("Markdown report: {}", path.display());
    }
    if let Some(path) = written.json_path {
        println!("JSON report: {}", path.display());
    }
}
