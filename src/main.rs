use anyhow::Result;
use clap::Parser;
use deepcode::cache::{cache_key, read_cached, write_cached};
use deepcode::cli::{Cli, Commands, Workflow};
use deepcode::code_structure::fill_missing_structure;
use deepcode::config::Config;
use deepcode::deepseek::DeepSeekClient;
use deepcode::diff::summarize_diff;
use deepcode::prompts::build_messages;
use deepcode::report::{parse_report, write_report, AnalysisReport, WrittenReport};
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
        Commands::Chat(cmd) => deepcode::tui::run_chat(cmd.path, &config, cli.no_cache),
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
    if let Some(max_tokens) = cli.max_tokens {
        config.max_tokens = max_tokens;
    }
    if let Some(thinking_enabled) = cli.thinking_enabled {
        config.thinking_enabled = thinking_enabled;
    }
    if let Some(reasoning_effort) = &cli.reasoning_effort {
        config.reasoning_effort = reasoning_effort.clone();
    }
    if let Some(retry_attempts) = cli.retry_attempts {
        config.retry_attempts = retry_attempts;
    }
    if let Some(retry_backoff_ms) = cli.retry_backoff_ms {
        config.retry_backoff_ms = retry_backoff_ms;
    }
    if let Some(api_timeout_secs) = cli.api_timeout_secs {
        config.api_timeout_secs = api_timeout_secs;
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
    if let Some(max_concurrency) = cli.max_concurrency {
        config.max_concurrency = max_concurrency;
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
    progress(&format!(
        "Scanning {} for {}",
        path.display(),
        workflow.as_str()
    ));
    let snapshot = scan_path(
        &path,
        ScanOptions {
            max_file_bytes: config.max_file_bytes,
            max_files: config.max_files,
            max_total_bytes: config.max_total_bytes,
            max_concurrency: config.max_concurrency,
        },
    )?;
    progress(&format!(
        "Scan complete: {} file(s), {} skipped, {} bytes, {} code lines",
        snapshot.summary.files_read,
        snapshot.summary.files_skipped,
        snapshot.summary.bytes_read,
        snapshot.summary.total_code_lines
    ));
    let report = ask_for_report(workflow, goal.as_deref(), config, no_cache, &snapshot)?;
    progress(&format!(
        "Writing report output to {}",
        config.output_dir.display()
    ));
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
    progress(&format!("Scanning old path {}", old_path.display()));
    let old_snapshot = scan_for_config(&old_path, config)?;
    progress(&format!(
        "Old scan complete: {} file(s), {} skipped",
        old_snapshot.summary.files_read, old_snapshot.summary.files_skipped
    ));
    progress(&format!("Scanning new path {}", new_path.display()));
    let new_snapshot = scan_for_config(&new_path, config)?;
    progress(&format!(
        "New scan complete: {} file(s), {} skipped",
        new_snapshot.summary.files_read, new_snapshot.summary.files_skipped
    ));
    let local_diff = summarize_diff(&old_snapshot, &new_snapshot);
    progress(&format!(
        "Local diff evidence: {} added, {} removed, {} modified, {} unchanged",
        local_diff.added.len(),
        local_diff.removed.len(),
        local_diff.modified.len(),
        local_diff.unchanged.len()
    ));
    let combined = combine_diff_snapshots(old_snapshot, new_snapshot);
    let goal = format!(
        "Compare old path {} to new path {}",
        old_path.display(),
        new_path.display()
    );
    let mut report = ask_for_report(Workflow::Diff, Some(&goal), config, no_cache, &combined)?;
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

    progress(&format!("Scanning {} for explore mode", path.display()));
    let snapshot = scan_for_config(&path, config)?;
    progress(&format!(
        "Scan complete: {} file(s), {} skipped, {} bytes",
        snapshot.summary.files_read, snapshot.summary.files_skipped, snapshot.summary.bytes_read
    ));
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
        let report = ask_for_report(
            Workflow::Explore,
            Some(question),
            config,
            no_cache,
            &snapshot,
        )?;
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
            max_concurrency: config.max_concurrency,
        },
    )
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
    progress("Building DeepSeek chat messages");
    let messages = build_messages(workflow, snapshot, goal);
    client.complete_with_progress(&messages, true, progress)
}

fn ask_for_report(
    workflow: Workflow,
    goal: Option<&str>,
    config: &Config,
    no_cache: bool,
    snapshot: &ProjectSnapshot,
) -> Result<AnalysisReport> {
    let raw = ask_model(workflow, goal, config, no_cache, snapshot)?;
    progress("Parsing DeepSeek JSON response");
    let mut report = parse_report(&raw)?;
    progress("Merging local structure evidence");
    fill_missing_structure(&mut report, snapshot);
    Ok(report)
}

fn ask_model(
    workflow: Workflow,
    goal: Option<&str>,
    config: &Config,
    no_cache: bool,
    snapshot: &ProjectSnapshot,
) -> Result<String> {
    if !config.cache_enabled || no_cache {
        progress("Cache disabled for this run");
        return request_model(workflow, goal, config, snapshot);
    }

    progress("Checking DeepSeek response cache");
    let key = cache_key(workflow, goal, config, snapshot)?;
    if let Some(content) = read_cached(config, &key)? {
        progress("Using cached DeepSeek response");
        return Ok(content);
    }

    progress("No cache entry found; requesting DeepSeek");
    let content = request_model(workflow, goal, config, snapshot)?;
    progress("Writing DeepSeek response to cache");
    write_cached(config, &key, &content)?;
    Ok(content)
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

fn progress(message: &str) {
    eprintln!("[deepcode] {message}");
}
