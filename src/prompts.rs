use crate::cli::Workflow;
use crate::deepseek::ChatMessage;
use crate::scanner::{ProjectSnapshot, ScannedFile};
use anyhow::{Context, Result};

const SYSTEM_PROMPT: &str = r#"You are DeepCode, a senior software engineering analysis agent.
You only analyze code that is provided in the prompt.
You do not claim to run, modify, or commit target project code.
Return valid JSON only. No Markdown fences, no prose outside JSON."#;

const SCHEMA_PROMPT: &str = r#"Return this exact JSON shape:
{
  "summary": "short project or file overview",
  "structure": {
    "entrypoints": ["relative entrypoint paths"],
    "modules": [
      {
        "path": "relative/path",
        "language": "language",
        "responsibility": "module responsibility",
        "symbols": ["important symbols"],
        "imports": ["important imports"]
      }
    ],
    "dependencies": [
      {
        "from": "relative/path",
        "to": "module or dependency",
        "kind": "import|runtime|data|test"
      }
    ]
  },
  "responsibilities": ["major responsibilities"],
  "core_components": [
    {
      "path": "relative/path",
      "name": "function/class/module/component",
      "role": "what it does",
      "inputs": ["inputs"],
      "outputs": ["outputs"],
      "dependencies": ["important dependencies"]
    }
  ],
  "quality": {
    "score": 0,
    "code_smells": ["specific smells"],
    "complexity_hotspots": ["specific hotspots"],
    "consistency": ["style/API/data consistency observations"],
    "security": ["security observations"],
    "maintainability": ["maintainability observations"]
  },
  "improvements": [
    {
      "title": "specific improvement",
      "rationale": "why it matters",
      "risk": "risk or tradeoff",
      "priority": "low|medium|high"
    }
  ],
  "tests": ["test suggestions"],
  "plan": [
    {
      "step": "implementation step",
      "reason": "why this step comes here",
      "verification": "how to verify"
    }
  ],
  "ideas": [
    {
      "title": "feature, performance, architecture, tech debt, or unconventional idea",
      "impact": "expected impact",
      "effort": "low|medium|high",
      "category": "feature|performance|architecture|tech-debt|creative"
    }
  ],
  "documents": [
    {
      "title": "document title",
      "kind": "readme|architecture|api|onboarding|changelog",
      "content": "complete document body in Markdown"
    }
  ],
  "diff": {
    "added": ["added files or behaviors"],
    "removed": ["removed files or behaviors"],
    "modified": ["modified files or behaviors"],
    "unchanged": ["unchanged files or behaviors"]
  },
  "risks": ["important risks or unknowns"]
}"#;

pub fn build_messages(
    workflow: Workflow,
    snapshot: &ProjectSnapshot,
    goal: Option<&str>,
) -> Result<Vec<ChatMessage>> {
    let user_prompt = build_user_prompt(workflow, snapshot, goal)?;
    Ok(vec![
        ChatMessage::system(format!("{SYSTEM_PROMPT}\n\n{SCHEMA_PROMPT}")),
        ChatMessage::user(user_prompt),
    ])
}

fn build_user_prompt(
    workflow: Workflow,
    snapshot: &ProjectSnapshot,
    goal: Option<&str>,
) -> Result<String> {
    let mut prompt = String::new();
    prompt.push_str(&format!("Workflow: {}\n", workflow.as_str()));
    prompt.push_str(&format!("Project root: {}\n", snapshot.root.display()));
    if let Some(goal) = goal {
        prompt.push_str(&format!("Goal: {goal}\n"));
    }
    prompt.push('\n');
    prompt.push_str(workflow_instruction(workflow));
    prompt.push('\n');
    prompt.push_str("Focus on concrete observations grounded in the provided files.\n");
    prompt.push_str("If a section is not relevant for this workflow, return an empty array or neutral score, but keep the schema intact.\n\n");
    prompt.push_str("Local scan summary:\n");
    prompt.push_str(&format!(
        "- files_read: {}\n- files_skipped: {}\n- bytes_read: {}\n- total_lines: {}\n- total_code_lines: {}\n",
        snapshot.summary.files_read,
        snapshot.summary.files_skipped,
        snapshot.summary.bytes_read,
        snapshot.summary.total_lines,
        snapshot.summary.total_code_lines
    ));
    if !snapshot.summary.languages.is_empty() {
        prompt.push_str("- languages:\n");
        for language in &snapshot.summary.languages {
            prompt.push_str(&format!(
                "  - {}: {} files, {} code lines, {} bytes\n",
                language.language, language.files, language.code_lines, language.bytes
            ));
        }
    }
    prompt.push('\n');
    prompt.push_str("Scanned files:\n");
    for file in &snapshot.files {
        append_file(&mut prompt, file);
    }
    if !snapshot.skipped.is_empty() {
        prompt.push_str("\nSkipped files:\n");
        for skipped in &snapshot.skipped {
            prompt.push_str(&format!(
                "- {}: {}\n",
                skipped.path.display(),
                skipped.reason
            ));
        }
    }

    serde_json::to_string(&snapshot.files).context("failed to serialize scanned files")?;
    Ok(prompt)
}

fn workflow_instruction(workflow: Workflow) -> &'static str {
    match workflow {
        Workflow::Summarize => {
            "Task: explain file responsibilities, core functions/classes/modules, inputs, outputs, dependencies, potential problems, and improvement suggestions."
        }
        Workflow::Analyze => {
            "Task: produce a quality report covering code smells, complexity hotspots, consistency, security, maintainability score, refactoring suggestions, and test suggestions."
        }
        Workflow::Plan => {
            "Task: generate a development plan based on the current project state and the provided goal. Include verification steps."
        }
        Workflow::Ideas => {
            "Task: suggest new features, performance optimizations, architecture evolution, technical debt fixes, and unconventional ideas."
        }
        Workflow::Report => {
            "Task: generate a comprehensive report combining understanding, quality analysis, planning recommendations, ideas, risks, and tests."
        }
        Workflow::Understand => {
            "Task: produce a structured code understanding map: entrypoints, modules, responsibilities, symbols, imports, dependency edges, data/control flow, and important unknowns."
        }
        Workflow::Docs => {
            "Task: generate multiple practical Markdown documents in the documents array: README, architecture guide, API/reference notes when applicable, onboarding guide, and changelog-style summary."
        }
        Workflow::Refactor => {
            "Task: generate a prioritized refactoring and improvement plan with dependencies, risk, verification, rollout order, and concrete tests."
        }
        Workflow::Diff => {
            "Task: compare the two provided snapshots. Fill diff added/removed/modified/unchanged and explain behavior, architecture, risk, and test impact."
        }
        Workflow::Explore => {
            "Task: answer the user's exploration question using the provided code context. Prefer concise, source-grounded answers and include follow-up questions when useful."
        }
        Workflow::Chat => {
            "Task: answer the user's chat question in Chinese using the provided code context. Be concrete, source-grounded, and preserve file paths exactly."
        }
    }
}

fn append_file(prompt: &mut String, file: &ScannedFile) {
    prompt.push_str(&format!(
        "\n--- FILE: {} | language: {} | bytes: {} | truncated: {} ---\n",
        file.path.display(),
        file.language,
        file.bytes,
        file.truncated
    ));
    prompt.push_str(&format!(
        "[metrics: lines={}, code_lines={}, comment_lines={}, blank_lines={}, longest_line={}]\n",
        file.metrics.lines,
        file.metrics.code_lines,
        file.metrics.comment_lines,
        file.metrics.blank_lines,
        file.metrics.longest_line
    ));
    prompt.push_str(&file.content);
    if !file.content.ends_with('\n') {
        prompt.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{ProjectSnapshot, ScannedFile};
    use std::path::PathBuf;

    #[test]
    fn builds_plan_prompt_with_goal_and_schema() {
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

        let messages = build_messages(Workflow::Plan, &snapshot, Some("add auth")).unwrap();

        assert_eq!(messages.len(), 2);
        assert!(messages[0].content.contains("Return valid JSON only"));
        assert!(messages[0].content.contains("\"quality\""));
        assert!(messages[1].content.contains("Workflow: plan"));
        assert!(messages[1].content.contains("Local scan summary"));
        assert!(messages[1].content.contains("Goal: add auth"));
        assert!(messages[1].content.contains("--- FILE: src/main.rs"));
    }
}
