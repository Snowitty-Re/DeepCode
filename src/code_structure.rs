use crate::report::{CodeStructure, DependencyEdge, StructureModule};
use crate::scanner::{ProjectSnapshot, ScannedFile};

pub fn infer_structure(snapshot: &ProjectSnapshot) -> CodeStructure {
    let modules = snapshot.files.iter().map(infer_module).collect::<Vec<_>>();
    let entrypoints = snapshot
        .files
        .iter()
        .filter(|file| is_entrypoint(file))
        .map(|file| file.path.display().to_string())
        .collect::<Vec<_>>();
    let dependencies = modules
        .iter()
        .flat_map(|module| {
            module.imports.iter().map(|import| DependencyEdge {
                from: module.path.clone(),
                to: import.clone(),
                kind: "import".to_string(),
            })
        })
        .collect();

    CodeStructure {
        entrypoints,
        modules,
        dependencies,
    }
}

fn infer_module(file: &ScannedFile) -> StructureModule {
    StructureModule {
        path: file.path.display().to_string(),
        language: file.language.clone(),
        responsibility: infer_responsibility(file),
        symbols: extract_symbols(file),
        imports: extract_imports(file),
    }
}

fn infer_responsibility(file: &ScannedFile) -> String {
    let path = file.path.to_string_lossy().to_lowercase();
    if path.contains("main") || path.contains("cli") {
        "Application entrypoint or command-line coordination".to_string()
    } else if path.contains("test") || path.contains("spec") {
        "Test coverage and verification".to_string()
    } else if path.contains("config") {
        "Configuration loading and validation".to_string()
    } else if path.contains("client") || path.contains("api") {
        "External API or service integration".to_string()
    } else if path.contains("report") || path.contains("doc") {
        "Report or documentation generation".to_string()
    } else {
        "Project module inferred from file content".to_string()
    }
}

fn is_entrypoint(file: &ScannedFile) -> bool {
    let path = file.path.to_string_lossy();
    path.ends_with("main.rs")
        || path.ends_with("main.go")
        || path.ends_with("index.js")
        || path.ends_with("index.ts")
        || path.ends_with("app.py")
        || path.ends_with("main.py")
        || file.content.contains("fn main(")
}

fn extract_symbols(file: &ScannedFile) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in file.content.lines() {
        let trimmed = line.trim_start();
        if let Some(symbol) = extract_after_keyword(trimmed, "fn ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "pub fn ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "struct ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "pub struct ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "class ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "def ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_after_keyword(trimmed, "function ") {
            symbols.push(symbol);
        } else if let Some(symbol) = extract_const_symbol(trimmed) {
            symbols.push(symbol);
        }
    }
    symbols.sort();
    symbols.dedup();
    symbols
}

fn extract_imports(file: &ScannedFile) -> Vec<String> {
    let mut imports = Vec::new();
    for line in file.content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("use ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("mod ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("import ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("from ") {
            imports.push(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("require(") {
            imports.push(
                value
                    .trim_matches(|c| c == ')' || c == '"' || c == '\'')
                    .to_string(),
            );
        }
    }
    imports.sort();
    imports.dedup();
    imports
}

fn extract_after_keyword(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?;
    let symbol = rest
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .next()?;
    (!symbol.is_empty()).then(|| symbol.to_string())
}

fn extract_const_symbol(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("let "))
        .or_else(|| line.strip_prefix("pub const "))?;
    let symbol = rest
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .next()?;
    (!symbol.is_empty()).then(|| symbol.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{FileMetrics, LanguageSummary, ScanSummary};
    use std::path::PathBuf;

    #[test]
    fn extracts_symbols_imports_and_entrypoints() {
        let snapshot = ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files: vec![ScannedFile {
                path: PathBuf::from("src/main.rs"),
                language: "Rust".to_string(),
                bytes: 32,
                truncated: false,
                metrics: FileMetrics {
                    lines: 3,
                    code_lines: 3,
                    comment_lines: 0,
                    blank_lines: 0,
                    longest_line: 16,
                },
                content: "use std::fs;\nstruct App {}\nfn main() {}\n".to_string(),
            }],
            skipped: vec![],
            summary: ScanSummary {
                files_read: 1,
                files_skipped: 0,
                bytes_read: 32,
                total_lines: 3,
                total_code_lines: 3,
                languages: vec![LanguageSummary {
                    language: "Rust".to_string(),
                    files: 1,
                    bytes: 32,
                    code_lines: 3,
                }],
            },
        };

        let structure = infer_structure(&snapshot);

        assert_eq!(structure.entrypoints, vec!["src/main.rs"]);
        assert!(structure.modules[0].symbols.contains(&"main".to_string()));
        assert!(structure.modules[0].symbols.contains(&"App".to_string()));
        assert_eq!(structure.dependencies[0].to, "std::fs");
    }
}
