use crate::cli::Workflow;
use crate::config::{Config, ReportFormat};
use crate::scanner::ProjectSnapshot;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisReport {
    pub summary: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub core_components: Vec<CoreComponent>,
    #[serde(default)]
    pub quality: QualityReport,
    #[serde(default)]
    pub improvements: Vec<Improvement>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub plan: Vec<PlanStep>,
    #[serde(default)]
    pub ideas: Vec<Idea>,
    #[serde(default)]
    pub risks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreComponent {
    pub path: String,
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QualityReport {
    pub score: u8,
    #[serde(default)]
    pub code_smells: Vec<String>,
    #[serde(default)]
    pub complexity_hotspots: Vec<String>,
    #[serde(default)]
    pub consistency: Vec<String>,
    #[serde(default)]
    pub security: Vec<String>,
    #[serde(default)]
    pub maintainability: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Improvement {
    pub title: String,
    pub rationale: String,
    pub risk: String,
    pub priority: Priority,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanStep {
    pub step: String,
    pub reason: String,
    pub verification: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Idea {
    pub title: String,
    pub impact: String,
    pub effort: Effort,
    pub category: IdeaCategory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IdeaCategory {
    Feature,
    Performance,
    Architecture,
    TechDebt,
    Creative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenReport {
    pub markdown_path: Option<PathBuf>,
    pub json_path: Option<PathBuf>,
}

impl Default for QualityReport {
    fn default() -> Self {
        Self {
            score: 0,
            code_smells: Vec::new(),
            complexity_hotspots: Vec::new(),
            consistency: Vec::new(),
            security: Vec::new(),
            maintainability: Vec::new(),
        }
    }
}

pub fn parse_report(content: &str) -> Result<AnalysisReport> {
    let value: Value = serde_json::from_str(content).context("model did not return valid JSON")?;
    serde_json::from_value(value).context("model JSON did not match DeepCode report schema")
}

pub fn write_report(
    config: &Config,
    workflow: Workflow,
    snapshot: &ProjectSnapshot,
    report: &AnalysisReport,
) -> Result<WrittenReport> {
    write_report_with_format(
        &config.output_dir,
        config.format,
        workflow,
        snapshot,
        report,
    )
}

pub fn write_report_with_format(
    output_dir: impl Into<PathBuf>,
    format: ReportFormat,
    workflow: Workflow,
    snapshot: &ProjectSnapshot,
    report: &AnalysisReport,
) -> Result<WrittenReport> {
    let output_dir = output_dir.into();
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create output directory {}", output_dir.display()))?;

    let base_name = format!("{}-{}", workflow.as_str(), timestamp());
    let markdown_path = if format.writes_markdown() {
        let path = output_dir.join(format!("{base_name}.md"));
        fs::write(&path, render_markdown(workflow, snapshot, report))
            .with_context(|| format!("failed to write Markdown report {}", path.display()))?;
        Some(path)
    } else {
        None
    };

    let json_path = if format.writes_json() {
        let path = output_dir.join(format!("{base_name}.json"));
        let json =
            serde_json::to_string_pretty(report).context("failed to serialize report JSON")?;
        fs::write(&path, json)
            .with_context(|| format!("failed to write JSON report {}", path.display()))?;
        Some(path)
    } else {
        None
    };

    Ok(WrittenReport {
        markdown_path,
        json_path,
    })
}

pub fn render_markdown(
    workflow: Workflow,
    snapshot: &ProjectSnapshot,
    report: &AnalysisReport,
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&format!("# DeepCode {} Report\n\n", workflow.as_str()));
    markdown.push_str(&format!("Root: `{}`\n\n", snapshot.root.display()));
    markdown.push_str("## Summary\n\n");
    markdown.push_str(&report.summary);
    markdown.push_str("\n\n");
    append_list(&mut markdown, "Responsibilities", &report.responsibilities);
    append_components(&mut markdown, &report.core_components);
    append_quality(&mut markdown, &report.quality);
    append_improvements(&mut markdown, &report.improvements);
    append_list(&mut markdown, "Tests", &report.tests);
    append_plan(&mut markdown, &report.plan);
    append_ideas(&mut markdown, &report.ideas);
    append_list(&mut markdown, "Risks", &report.risks);
    markdown.push_str("## Scan\n\n");
    markdown.push_str(&format!(
        "- Files read: {}\n- Files skipped: {}\n- Bytes read: {}\n- Total lines: {}\n- Code lines: {}\n",
        snapshot.summary.files_read,
        snapshot.summary.files_skipped,
        snapshot.summary.bytes_read,
        snapshot.summary.total_lines,
        snapshot.summary.total_code_lines
    ));
    if !snapshot.summary.languages.is_empty() {
        markdown.push_str("\n### Languages\n\n");
        for language in &snapshot.summary.languages {
            markdown.push_str(&format!(
                "- {}: {} files, {} code lines, {} bytes\n",
                language.language, language.files, language.code_lines, language.bytes
            ));
        }
    }
    markdown
}

fn append_list(markdown: &mut String, title: &str, items: &[String]) {
    markdown.push_str(&format!("## {title}\n\n"));
    if items.is_empty() {
        markdown.push_str("- None\n\n");
        return;
    }
    for item in items {
        markdown.push_str(&format!("- {item}\n"));
    }
    markdown.push('\n');
}

fn append_components(markdown: &mut String, components: &[CoreComponent]) {
    markdown.push_str("## Core Components\n\n");
    if components.is_empty() {
        markdown.push_str("- None\n\n");
        return;
    }
    for component in components {
        markdown.push_str(&format!(
            "- `{}` `{}`: {}\n",
            component.path, component.name, component.role
        ));
        append_inline_list(markdown, "Inputs", &component.inputs);
        append_inline_list(markdown, "Outputs", &component.outputs);
        append_inline_list(markdown, "Dependencies", &component.dependencies);
    }
    markdown.push('\n');
}

fn append_quality(markdown: &mut String, quality: &QualityReport) {
    markdown.push_str("## Quality\n\n");
    markdown.push_str(&format!("Score: `{}/100`\n\n", quality.score));
    append_list(markdown, "Code Smells", &quality.code_smells);
    append_list(
        markdown,
        "Complexity Hotspots",
        &quality.complexity_hotspots,
    );
    append_list(markdown, "Consistency", &quality.consistency);
    append_list(markdown, "Security", &quality.security);
    append_list(markdown, "Maintainability", &quality.maintainability);
}

fn append_improvements(markdown: &mut String, improvements: &[Improvement]) {
    markdown.push_str("## Improvements\n\n");
    if improvements.is_empty() {
        markdown.push_str("- None\n\n");
        return;
    }
    for improvement in improvements {
        markdown.push_str(&format!(
            "- **{}** [{:?}]: {} Risk: {}\n",
            improvement.title, improvement.priority, improvement.rationale, improvement.risk
        ));
    }
    markdown.push('\n');
}

fn append_plan(markdown: &mut String, plan: &[PlanStep]) {
    markdown.push_str("## Plan\n\n");
    if plan.is_empty() {
        markdown.push_str("- None\n\n");
        return;
    }
    for (index, step) in plan.iter().enumerate() {
        markdown.push_str(&format!(
            "{}. {} Reason: {} Verification: {}\n",
            index + 1,
            step.step,
            step.reason,
            step.verification
        ));
    }
    markdown.push('\n');
}

fn append_ideas(markdown: &mut String, ideas: &[Idea]) {
    markdown.push_str("## Ideas\n\n");
    if ideas.is_empty() {
        markdown.push_str("- None\n\n");
        return;
    }
    for idea in ideas {
        markdown.push_str(&format!(
            "- **{}** [{:?}/{:?}]: {}\n",
            idea.title, idea.category, idea.effort, idea.impact
        ));
    }
    markdown.push('\n');
}

fn append_inline_list(markdown: &mut String, label: &str, items: &[String]) {
    if !items.is_empty() {
        markdown.push_str(&format!("  - {label}: {}\n", items.join(", ")));
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{ProjectSnapshot, ScannedFile};

    #[test]
    fn parses_report_schema() {
        let report = parse_report(
            r#"{
              "summary": "Small app",
              "responsibilities": ["run"],
              "core_components": [],
              "quality": {"score": 80},
              "improvements": [],
              "tests": [],
              "plan": [],
              "ideas": [],
              "risks": []
            }"#,
        )
        .unwrap();

        assert_eq!(report.summary, "Small app");
        assert_eq!(report.quality.score, 80);
    }

    #[test]
    fn renders_markdown_report() {
        let snapshot = ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files: vec![ScannedFile {
                path: PathBuf::from("src/main.rs"),
                language: "Rust".to_string(),
                bytes: 12,
                truncated: false,
                metrics: crate::scanner::FileMetrics {
                    lines: 1,
                    code_lines: 1,
                    comment_lines: 0,
                    blank_lines: 0,
                    longest_line: 12,
                },
                content: "fn main() {}".to_string(),
            }],
            skipped: vec![],
            summary: crate::scanner::ScanSummary {
                files_read: 1,
                files_skipped: 0,
                bytes_read: 12,
                total_lines: 1,
                total_code_lines: 1,
                languages: vec![crate::scanner::LanguageSummary {
                    language: "Rust".to_string(),
                    files: 1,
                    bytes: 12,
                    code_lines: 1,
                }],
            },
        };
        let report = AnalysisReport {
            summary: "Small app".to_string(),
            responsibilities: vec!["run".to_string()],
            core_components: vec![],
            quality: QualityReport {
                score: 80,
                ..QualityReport::default()
            },
            improvements: vec![],
            tests: vec!["cargo test".to_string()],
            plan: vec![],
            ideas: vec![],
            risks: vec![],
        };

        let markdown = render_markdown(Workflow::Report, &snapshot, &report);

        assert!(markdown.contains("# DeepCode report Report"));
        assert!(markdown.contains("Small app"));
        assert!(markdown.contains("Files read: 1"));
    }
}
