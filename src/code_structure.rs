use crate::report::{
    AnalysisReport, CodeStructure, DependencyEdge, StructureModule, StructureSymbol,
};
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

pub fn fill_missing_structure(report: &mut AnalysisReport, snapshot: &ProjectSnapshot) {
    let local = infer_structure(snapshot);
    merge_unique_strings(&mut report.structure.entrypoints, &local.entrypoints);
    if report.structure.modules.is_empty() {
        report.structure.modules = local.modules.clone();
    } else {
        merge_local_module_evidence(&mut report.structure.modules, &local.modules);
    }
    merge_unique_dependencies(&mut report.structure.dependencies, &local.dependencies);
}

fn merge_local_module_evidence(
    modules: &mut Vec<StructureModule>,
    local_modules: &[StructureModule],
) {
    for local in local_modules {
        let Some(module) = modules.iter_mut().find(|module| module.path == local.path) else {
            modules.push(local.clone());
            continue;
        };
        if module.symbols.is_empty() {
            module.symbols = local.symbols.clone();
        }
        if module.symbol_details.is_empty() {
            module.symbol_details = local.symbol_details.clone();
        }
        if module.imports.is_empty() {
            module.imports = local.imports.clone();
        }
        if module.responsibility.trim().is_empty() {
            module.responsibility = local.responsibility.clone();
        }
    }
}

fn merge_unique_strings(target: &mut Vec<String>, source: &[String]) {
    for value in source {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn merge_unique_dependencies(target: &mut Vec<DependencyEdge>, source: &[DependencyEdge]) {
    for dependency in source {
        if !target.iter().any(|existing| {
            existing.from == dependency.from
                && existing.to == dependency.to
                && existing.kind == dependency.kind
        }) {
            target.push(dependency.clone());
        }
    }
}

fn infer_module(file: &ScannedFile) -> StructureModule {
    let symbol_details = extract_symbol_details(file);
    let symbols = symbol_names(&symbol_details);
    StructureModule {
        path: file.path.display().to_string(),
        language: file.language.clone(),
        responsibility: infer_responsibility(file),
        symbols,
        symbol_details,
        imports: extract_imports(file),
    }
}

fn infer_responsibility(file: &ScannedFile) -> String {
    let path = file.path.to_string_lossy().to_lowercase();
    let imports = extract_imports(file);
    if is_entrypoint(file) || path.contains("main") || path.contains("cli") {
        "Application entrypoint or command-line coordination".to_string()
    } else if path.contains("test") || path.contains("spec") {
        "Test coverage and verification".to_string()
    } else if path.contains("config") {
        "Configuration loading and validation".to_string()
    } else if path.contains("client")
        || path.contains("api")
        || imports.iter().any(|import| {
            import.contains("reqwest")
                || import.contains("hyper")
                || import.contains("axios")
                || import.contains("fetch")
                || import.contains("requests")
        })
    {
        "External API or service integration".to_string()
    } else if path.contains("report") || path.contains("doc") {
        "Report or documentation generation".to_string()
    } else if path.contains("scan") || path.contains("walk") {
        "Source discovery, scanning, or indexing".to_string()
    } else if path.contains("cache") {
        "Caching and reuse of derived analysis data".to_string()
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
        || path.ends_with("index.tsx")
        || path.ends_with("index.jsx")
        || path.ends_with("app.py")
        || path.ends_with("main.py")
        || path.ends_with("package.json")
        || path.ends_with("Cargo.toml")
        || file.content.contains("fn main(")
        || file.content.contains("package main")
        || file.content.contains("if __name__ == \"__main__\"")
}

fn extract_symbol_details(file: &ScannedFile) -> Vec<StructureSymbol> {
    let mut symbols = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_start();
        if should_skip_symbol_line(trimmed) {
            continue;
        }
        if let Some(symbol) = detect_symbol(trimmed, line_number, file.language.as_str()) {
            symbols.push(symbol);
        }
    }
    symbols.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
    });
    symbols.dedup();
    symbols
}

fn symbol_names(symbols: &[StructureSymbol]) -> Vec<String> {
    let mut names = symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn extract_imports(file: &ScannedFile) -> Vec<String> {
    let mut imports = Vec::new();
    for line in file.content.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("use ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("mod ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("pub mod ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("import ") {
            imports.push(value.trim_end_matches(';').to_string());
        } else if let Some(value) = trimmed.strip_prefix("from ") {
            imports.push(value.to_string());
        } else if let Some(value) = trimmed.strip_prefix("package ") {
            imports.push(format!("package {}", value.trim_end_matches(';')));
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

fn should_skip_symbol_line(line: &str) -> bool {
    line.is_empty()
        || line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with("/*")
        || line.starts_with('*')
        || line.starts_with("*/")
}

fn detect_symbol(line: &str, line_number: usize, language: &str) -> Option<StructureSymbol> {
    match language {
        "Rust" => detect_rust_symbol(line, line_number),
        "TypeScript" | "JavaScript" | "JavaScript JSX" => detect_js_symbol(line, line_number),
        "Python" => detect_python_symbol(line, line_number),
        "Go" => detect_go_symbol(line, line_number),
        _ => detect_generic_symbol(line, line_number),
    }
}

fn detect_rust_symbol(line: &str, line_number: usize) -> Option<StructureSymbol> {
    let (visibility, rest) = rust_visibility(line);
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    if let Some(name) = extract_after_keyword(rest, "fn ") {
        return Some(symbol(name, "function", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "struct ") {
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "enum ") {
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "trait ") {
        return Some(symbol(name, "trait", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "mod ") {
        return Some(symbol(name, "module", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "type ") {
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "const ") {
        return Some(symbol(name, "constant", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "static ") {
        return Some(symbol(name, "constant", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "macro_rules! ") {
        return Some(symbol(name, "macro", visibility, line_number));
    }
    if let Some(name) = extract_impl_symbol(rest) {
        return Some(symbol(name, "impl", visibility, line_number));
    }
    None
}

fn rust_visibility(line: &str) -> (&'static str, &str) {
    if let Some(rest) = line.strip_prefix("pub ") {
        return ("public", rest);
    }
    if let Some(rest) = line.strip_prefix("pub(") {
        if let Some(end) = rest.find(')') {
            return ("restricted", rest[end + 1..].trim_start());
        }
    }
    ("private", line)
}

fn extract_impl_symbol(line: &str) -> Option<String> {
    let rest = line.strip_prefix("impl ")?;
    let target = rest
        .split_once(" for ")
        .map(|(_, target)| target)
        .unwrap_or(rest)
        .trim();
    let name = target
        .split(|character: char| {
            character.is_whitespace()
                || character == '<'
                || character == '{'
                || character == '('
                || character == ':'
        })
        .next()?;
    (!name.is_empty()).then(|| format!("impl {name}"))
}

fn detect_js_symbol(line: &str, line_number: usize) -> Option<StructureSymbol> {
    let (visibility, rest) = js_visibility(line);
    if let Some(name) = extract_after_keyword(rest, "async function ") {
        return Some(symbol(name, "function", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "function ") {
        return Some(symbol(name, "function", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "class ") {
        return Some(symbol(name, "class", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "interface ") {
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(rest, "type ") {
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_assignment_symbol(rest) {
        return Some(symbol(name, "variable", visibility, line_number));
    }
    None
}

fn js_visibility(line: &str) -> (&'static str, &str) {
    if let Some(rest) = line.strip_prefix("export default ") {
        return ("exported", rest);
    }
    if let Some(rest) = line.strip_prefix("export ") {
        return ("exported", rest);
    }
    ("private", line)
}

fn detect_python_symbol(line: &str, line_number: usize) -> Option<StructureSymbol> {
    if let Some(name) = extract_after_keyword(line, "async def ") {
        return Some(symbol(name, "function", "private", line_number));
    }
    if let Some(name) = extract_after_keyword(line, "def ") {
        return Some(symbol(name, "function", "private", line_number));
    }
    if let Some(name) = extract_after_keyword(line, "class ") {
        return Some(symbol(name, "class", "private", line_number));
    }
    None
}

fn detect_go_symbol(line: &str, line_number: usize) -> Option<StructureSymbol> {
    if let Some(name) = extract_after_keyword(line, "func ") {
        let visibility = go_visibility(&name);
        return Some(symbol(name, "function", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(line, "type ") {
        let visibility = go_visibility(&name);
        return Some(symbol(name, "type", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(line, "const ") {
        let visibility = go_visibility(&name);
        return Some(symbol(name, "constant", visibility, line_number));
    }
    if let Some(name) = extract_after_keyword(line, "var ") {
        let visibility = go_visibility(&name);
        return Some(symbol(name, "variable", visibility, line_number));
    }
    None
}

fn go_visibility(name: &str) -> &'static str {
    if name.chars().next().is_some_and(char::is_uppercase) {
        "exported"
    } else {
        "private"
    }
}

fn detect_generic_symbol(line: &str, line_number: usize) -> Option<StructureSymbol> {
    if let Some(name) = extract_after_keyword(line, "fn ") {
        return Some(symbol(name, "function", "unknown", line_number));
    }
    if let Some(name) = extract_after_keyword(line, "function ") {
        return Some(symbol(name, "function", "unknown", line_number));
    }
    if let Some(name) = extract_after_keyword(line, "class ") {
        return Some(symbol(name, "class", "unknown", line_number));
    }
    if let Some(name) = extract_after_keyword(line, "def ") {
        return Some(symbol(name, "function", "unknown", line_number));
    }
    None
}

fn extract_after_keyword(line: &str, keyword: &str) -> Option<String> {
    let rest = line.strip_prefix(keyword)?;
    extract_identifier(rest)
}

fn extract_assignment_symbol(line: &str) -> Option<String> {
    let rest = line
        .strip_prefix("const ")
        .or_else(|| line.strip_prefix("let "))
        .or_else(|| line.strip_prefix("var "))?;
    extract_identifier(rest)
}

fn extract_identifier(value: &str) -> Option<String> {
    let symbol = value
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .next()?;
    (!symbol.is_empty()).then(|| symbol.to_string())
}

fn symbol(
    name: String,
    kind: impl Into<String>,
    visibility: impl Into<String>,
    line: usize,
) -> StructureSymbol {
    StructureSymbol {
        name,
        kind: kind.into(),
        visibility: visibility.into(),
        line,
    }
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
        assert_eq!(structure.modules[0].symbol_details[0].name, "App");
        assert_eq!(structure.modules[0].symbol_details[0].kind, "type");
        assert_eq!(structure.modules[0].symbol_details[0].visibility, "private");
        assert_eq!(structure.dependencies[0].to, "std::fs");
    }

    #[test]
    fn extracts_structured_symbols_across_common_languages() {
        let snapshot = ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files: vec![
                ScannedFile {
                    path: PathBuf::from("src/lib.rs"),
                    language: "Rust".to_string(),
                    bytes: 120,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 5,
                        code_lines: 5,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 32,
                    },
                    content:
                        "pub struct Client {}\npub(crate) async fn run() {}\ntrait Store {}\nimpl Client {}\n"
                            .to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("src/index.ts"),
                    language: "TypeScript".to_string(),
                    bytes: 90,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 3,
                        code_lines: 3,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 32,
                    },
                    content: "export class App {}\nconst value = 1\nexport function boot() {}\n"
                        .to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("main.py"),
                    language: "Python".to_string(),
                    bytes: 60,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 2,
                        code_lines: 2,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 20,
                    },
                    content: "class Worker:\nasync def run(): pass\n".to_string(),
                },
            ],
            skipped: vec![],
            summary: ScanSummary::default(),
        };

        let structure = infer_structure(&snapshot);
        let rust = &structure.modules[0].symbol_details;
        let typescript = &structure.modules[1].symbol_details;
        let python = &structure.modules[2].symbol_details;

        assert!(rust.iter().any(|symbol| symbol.name == "Client"
            && symbol.kind == "type"
            && symbol.visibility == "public"));
        assert!(rust.iter().any(|symbol| symbol.name == "run"
            && symbol.kind == "function"
            && symbol.visibility == "restricted"));
        assert!(rust
            .iter()
            .any(|symbol| symbol.name == "impl Client" && symbol.kind == "impl"));
        assert!(typescript.iter().any(|symbol| symbol.name == "App"
            && symbol.kind == "class"
            && symbol.visibility == "exported"));
        assert!(typescript
            .iter()
            .any(|symbol| symbol.name == "value" && symbol.kind == "variable"));
        assert!(python
            .iter()
            .any(|symbol| symbol.name == "Worker" && symbol.kind == "class"));
        assert!(structure.entrypoints.contains(&"src/index.ts".to_string()));
        assert!(structure.entrypoints.contains(&"main.py".to_string()));
    }

    #[test]
    fn merges_local_evidence_into_existing_model_structure() {
        let snapshot = ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files: vec![ScannedFile {
                path: PathBuf::from("src/main.rs"),
                language: "Rust".to_string(),
                bytes: 40,
                truncated: false,
                metrics: FileMetrics {
                    lines: 2,
                    code_lines: 2,
                    comment_lines: 0,
                    blank_lines: 0,
                    longest_line: 16,
                },
                content: "use std::fs;\nfn main() {}\n".to_string(),
            }],
            skipped: vec![],
            summary: ScanSummary::default(),
        };
        let mut report = AnalysisReport {
            summary: "Model summary".to_string(),
            structure: CodeStructure {
                entrypoints: vec!["model-entry.rs".to_string()],
                modules: vec![StructureModule {
                    path: "src/main.rs".to_string(),
                    language: "Rust".to_string(),
                    responsibility: "Model responsibility".to_string(),
                    symbols: vec![],
                    symbol_details: vec![],
                    imports: vec![],
                }],
                dependencies: vec![],
            },
            responsibilities: vec![],
            core_components: vec![],
            quality: crate::report::QualityReport::default(),
            improvements: vec![],
            tests: vec![],
            plan: vec![],
            ideas: vec![],
            documents: vec![],
            diff: crate::report::DiffSummary::default(),
            risks: vec![],
        };

        fill_missing_structure(&mut report, &snapshot);

        assert!(report
            .structure
            .entrypoints
            .contains(&"model-entry.rs".to_string()));
        assert!(report
            .structure
            .entrypoints
            .contains(&"src/main.rs".to_string()));
        assert!(report.structure.modules[0]
            .symbols
            .contains(&"main".to_string()));
        assert_eq!(report.structure.modules[0].symbol_details[0].line, 2);
        assert_eq!(report.structure.modules[0].imports, vec!["std::fs"]);
        assert_eq!(report.structure.dependencies[0].to, "std::fs");
    }
}
