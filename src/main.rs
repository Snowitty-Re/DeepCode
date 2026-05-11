use anyhow::Result;
use clap::Parser;
use deepcode::cache::{cache_key, read_cached, write_cached};
use deepcode::cli::{Cli, Commands, Workflow};
use deepcode::code_structure::infer_structure;
use deepcode::config::Config;
use deepcode::deepseek::DeepSeekClient;
use deepcode::diff::summarize_diff;
use deepcode::prompts::build_messages;
use deepcode::report::{parse_report, write_report, WrittenReport};
use deepcode::scanner::{scan_path, ProjectSnapshot, ScanOptions};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli)?;

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
        Commands::Understand(cmd) => {
            run_workflow(Workflow::Understand, cmd.path, None, &config, cli.no_cache)
        }
        Commands::Docs(cmd) => {
            let goal = docs_goal(&cmd.kinds);
            run_workflow(Workflow::Docs, cmd.path, Some(goal), &config, cli.no_cache)
        }
        Commands::Refactor(cmd) => run_workflow(
            Workflow::Refactor,
            cmd.path,
            Some(cmd.goal),
            &config,
            cli.no_cache,
        ),
        Commands::Diff(cmd) => run_diff(cmd.old_path, cmd.new_path, &config, cli.no_cache),
        Commands::Explore(cmd) => run_explore(cmd.path, &config, cli.no_cache),
    }
}

fn load_config(cli: &Cli) -> Result<Config> {
    let mut config = match &cli.config {
        Some(path) => Config::load_file(path)?,
        None => Config::load(std::env::current_dir()?)?,
    };
    if let Some(base_url) = &cli.base_url {
        config.base_url = base_url.clone();
    }
    if let Some(model) = &cli.model {
        config.model = model.clone();
    }
    if let Some(output_dir) = &cli.output_dir {
        config.output_dir = output_dir.clone();
    }
    if let Some(format) = cli.format {
        config.format = format.into();
    }
    if let Some(max_file_bytes) = cli.max_file_bytes {
        config.max_file_bytes = max_file_bytes;
    }
    if let Some(max_files) = cli.max_files {
        config.max_files = max_files;
    }
    if let Some(max_total_bytes) = cli.max_total_bytes {
        config.max_total_bytes = max_total_bytes;
    }
    config.validate_public()?;
    Ok(config)
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
            max_files: config.max_files,
            max_total_bytes: config.max_total_bytes,
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
    let mut report = parse_report(&raw)?;
    merge_local_structure(&mut report, &snapshot);
    let written = write_report(config, workflow, &snapshot, &report)?;
    print_written_report(written);
    Ok(())
}

fn run_diff(
    old_path: std::path::PathBuf,
    new_path: std::path::PathBuf,
    config: &Config,
    no_cache: bool,
) -> Result<()> {
    let old_snapshot = scan_for_config(&old_path, config)?;
    let new_snapshot = scan_for_config(&new_path, config)?;
    let local_diff = summarize_diff(&old_snapshot, &new_snapshot);
    let combined = combine_diff_snapshots(old_snapshot, new_snapshot);
    let goal = format!(
        "Compare old path {} to new path {}",
        old_path.display(),
        new_path.display()
    );
    let cache_enabled = config.cache_enabled && !no_cache;
    let key = cache_key(Workflow::Diff, Some(&goal), config, &combined)?;
    let raw = if cache_enabled {
        match read_cached(config, &key)? {
            Some(content) => {
                println!("Using cached DeepSeek response");
                content
            }
            None => {
                let content = request_model(Workflow::Diff, Some(&goal), config, &combined)?;
                write_cached(config, &key, &content)?;
                content
            }
        }
    } else {
        request_model(Workflow::Diff, Some(&goal), config, &combined)?
    };
    let mut report = parse_report(&raw)?;
    merge_local_structure(&mut report, &combined);
    if report.diff.added.is_empty()
        && report.diff.removed.is_empty()
        && report.diff.modified.is_empty()
        && report.diff.unchanged.is_empty()
    {
        report.diff = local_diff;
    }
    let written = write_report(config, Workflow::Diff, &combined, &report)?;
    print_written_report(written);
    Ok(())
}

fn run_explore(path: std::path::PathBuf, config: &Config, no_cache: bool) -> Result<()> {
    use std::io::{self, Write};

    let snapshot = scan_for_config(&path, config)?;
    println!("DeepCode explore mode. Type a question, or :quit to exit.");
    loop {
        print!("deepcode> ");
        io::stdout().flush()?;
        let mut question = String::new();
        if io::stdin().read_line(&mut question)? == 0 {
            break;
        }
        let question = question.trim();
        if question.is_empty() {
            continue;
        }
        if matches!(question, ":quit" | ":exit") {
            break;
        }
        let cache_enabled = config.cache_enabled && !no_cache;
        let key = cache_key(Workflow::Explore, Some(question), config, &snapshot)?;
        let raw = if cache_enabled {
            match read_cached(config, &key)? {
                Some(content) => content,
                None => {
                    let content =
                        request_model(Workflow::Explore, Some(question), config, &snapshot)?;
                    write_cached(config, &key, &content)?;
                    content
                }
            }
        } else {
            request_model(Workflow::Explore, Some(question), config, &snapshot)?
        };
        let mut report = parse_report(&raw)?;
        merge_local_structure(&mut report, &snapshot);
        println!("\n{}\n", report.summary);
        if !report.risks.is_empty() {
            println!("Risks:");
            for risk in &report.risks {
                println!("- {risk}");
            }
            println!();
        }
    }
    Ok(())
}

fn scan_for_config(path: &std::path::Path, config: &Config) -> Result<ProjectSnapshot> {
    scan_path(
        path,
        ScanOptions {
            max_file_bytes: config.max_file_bytes,
            max_files: config.max_files,
            max_total_bytes: config.max_total_bytes,
        },
    )
}

fn merge_local_structure(
    report: &mut deepcode::report::AnalysisReport,
    snapshot: &ProjectSnapshot,
) {
    let local = infer_structure(snapshot);
    if report.structure.entrypoints.is_empty() {
        report.structure.entrypoints = local.entrypoints;
    }
    if report.structure.modules.is_empty() {
        report.structure.modules = local.modules;
    }
    if report.structure.dependencies.is_empty() {
        report.structure.dependencies = local.dependencies;
    }
}

fn docs_goal(kinds: &[deepcode::cli::DocKind]) -> String {
    if kinds.is_empty() {
        "Generate README, architecture, API/reference, onboarding, and changelog documentation"
            .to_string()
    } else {
        let names = kinds
            .iter()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!("Generate these documentation kinds: {names}")
    }
}

fn combine_diff_snapshots(
    mut old_snapshot: ProjectSnapshot,
    mut new_snapshot: ProjectSnapshot,
) -> ProjectSnapshot {
    for file in &mut old_snapshot.files {
        file.path = std::path::PathBuf::from("old").join(&file.path);
    }
    for skipped in &mut old_snapshot.skipped {
        skipped.path = std::path::PathBuf::from("old").join(&skipped.path);
    }
    for file in &mut new_snapshot.files {
        file.path = std::path::PathBuf::from("new").join(&file.path);
    }
    for skipped in &mut new_snapshot.skipped {
        skipped.path = std::path::PathBuf::from("new").join(&skipped.path);
    }

    let mut files = old_snapshot.files;
    files.extend(new_snapshot.files);
    let mut skipped = old_snapshot.skipped;
    skipped.extend(new_snapshot.skipped);
    let summary = deepcode::scanner::ScanSummary {
        files_read: files.len(),
        files_skipped: skipped.len(),
        bytes_read: files.iter().map(|file| file.bytes).sum(),
        total_lines: files.iter().map(|file| file.metrics.lines).sum(),
        total_code_lines: files.iter().map(|file| file.metrics.code_lines).sum(),
        languages: Vec::new(),
    };

    ProjectSnapshot {
        root: std::path::PathBuf::from("diff"),
        files,
        skipped,
        summary,
    }
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
    for path in written.document_paths {
        println!("Generated document: {}", path.display());
    }
}
