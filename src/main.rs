use anyhow::Result;
use clap::Parser;
use deepcode::cli::{Cli, Commands};
use deepcode::config::Config;
use std::path::PathBuf;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load(std::env::current_dir()?)?;

    match cli.command {
        Commands::Summarize(cmd) => run_stub("summarize", cmd.path, None, &config),
        Commands::Analyze(cmd) => run_stub("analyze", cmd.path, None, &config),
        Commands::Plan(cmd) => run_stub("plan", cmd.path, Some(cmd.goal), &config),
        Commands::Ideas(cmd) => run_stub("ideas", cmd.path, None, &config),
        Commands::Report(cmd) => run_stub("report", cmd.path, None, &config),
    }
}

fn run_stub(command: &str, path: PathBuf, goal: Option<String>, config: &Config) -> Result<()> {
    println!("deepcode {command}: {}", path.display());
    if let Some(goal) = goal {
        println!("goal: {goal}");
    }
    println!("model: {}", config.model);
    println!("implementation in progress");
    Ok(())
}
