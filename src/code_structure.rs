use crate::report::{
    AnalysisReport, CodeStructure, DependencyEdge, StructureModule, StructureSymbol,
};
use crate::scanner::{ProjectSnapshot, ScannedFile};
use std::path::{Path, PathBuf};

pub fn infer_structure(snapshot: &ProjectSnapshot) -> CodeStructure {
    let modules = snapshot.files.iter().map(infer_module).collect::<Vec<_>>();
    let entrypoints = snapshot
        .files
        .iter()
        .filter(|file| is_entrypoint(file))
        .map(|file| file.path.display().to_string())
        .collect::<Vec<_>>();
    let dependencies = infer_dependencies(snapshot);

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
                && existing.target_type == dependency.target_type
                && existing.evidence == dependency.evidence
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedDependency {
    to: String,
    target_type: String,
}

fn infer_dependencies(snapshot: &ProjectSnapshot) -> Vec<DependencyEdge> {
    let known_paths = snapshot
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let mut dependencies = Vec::new();
    for file in &snapshot.files {
        for import in extract_imports(file) {
            let resolved = resolve_dependency(file, &import, &known_paths);
            dependencies.push(DependencyEdge {
                from: file.path.display().to_string(),
                to: resolved.to,
                kind: dependency_kind(file),
                target_type: resolved.target_type,
                evidence: import,
            });
        }
    }
    dependencies.sort_by(|left, right| {
        left.from
            .cmp(&right.from)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.target_type.cmp(&right.target_type))
            .then_with(|| left.to.cmp(&right.to))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    dependencies.dedup();
    dependencies
}

fn dependency_kind(file: &ScannedFile) -> String {
    if is_test_file(file.path.as_path()) {
        "test".to_string()
    } else {
        "import".to_string()
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
        if should_skip_symbol_line(trimmed) {
            continue;
        }
        match file.language.as_str() {
            "Rust" => extract_rust_import(trimmed, &mut imports),
            "TypeScript" | "JavaScript" | "JavaScript JSX" => {
                extract_js_import(trimmed, &mut imports)
            }
            "Python" => extract_python_import(trimmed, &mut imports),
            "Go" => extract_go_import(trimmed, &mut imports),
            _ => extract_generic_import(trimmed, &mut imports),
        }
    }
    imports.sort();
    imports.dedup();
    imports
}

fn extract_rust_import(line: &str, imports: &mut Vec<String>) {
    if let Some(value) = line.strip_prefix("use ") {
        imports.push(value.trim_end_matches(';').to_string());
    } else if let Some(value) = line.strip_prefix("mod ") {
        imports.push(value.trim_end_matches(';').to_string());
    } else if let Some(value) = line.strip_prefix("pub mod ") {
        imports.push(value.trim_end_matches(';').to_string());
    }
}

fn extract_js_import(line: &str, imports: &mut Vec<String>) {
    if let Some(value) = js_import_specifier(line) {
        imports.push(value);
    }
    if let Some(value) = require_specifier(line) {
        imports.push(value);
    }
}

fn js_import_specifier(line: &str) -> Option<String> {
    let value = line.strip_prefix("import ")?;
    if let Some((_, specifier)) = value.rsplit_once(" from ") {
        return quoted_specifier(specifier.trim_end_matches(';'));
    }
    quoted_specifier(value.trim_end_matches(';'))
}

fn require_specifier(line: &str) -> Option<String> {
    let start = line.find("require(")?;
    let value = &line[start + "require(".len()..];
    let end = value.find(')')?;
    quoted_specifier(&value[..end])
}

fn quoted_specifier(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let quote = trimmed.as_bytes()[0] as char;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &trimmed[1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn extract_python_import(line: &str, imports: &mut Vec<String>) {
    if let Some(value) = line.strip_prefix("from ") {
        let module = value.split(" import ").next().unwrap_or(value).trim();
        if !module.is_empty() {
            imports.push(module.to_string());
        }
    } else if let Some(value) = line.strip_prefix("import ") {
        for module in value.split(',') {
            let module = module.split_whitespace().next().unwrap_or_default();
            if !module.is_empty() {
                imports.push(module.to_string());
            }
        }
    }
}

fn extract_go_import(line: &str, imports: &mut Vec<String>) {
    if let Some(value) = line.strip_prefix("import ") {
        if let Some(specifier) = quoted_specifier(value) {
            imports.push(specifier);
        }
    } else if let Some(specifier) = quoted_specifier(line) {
        imports.push(specifier);
    }
}

fn extract_generic_import(line: &str, imports: &mut Vec<String>) {
    if let Some(value) = line.strip_prefix("import ") {
        imports.push(value.trim_end_matches(';').to_string());
    } else if let Some(value) = line.strip_prefix("from ") {
        imports.push(value.to_string());
    } else if let Some(value) = require_specifier(line) {
        imports.push(value);
    }
}

fn resolve_dependency(
    file: &ScannedFile,
    import: &str,
    known_paths: &[PathBuf],
) -> ResolvedDependency {
    match file.language.as_str() {
        "Rust" => resolve_rust_dependency(file.path.as_path(), import, known_paths),
        "TypeScript" | "JavaScript" | "JavaScript JSX" => {
            resolve_js_dependency(file.path.as_path(), import, known_paths)
        }
        "Python" => resolve_python_dependency(file.path.as_path(), import, known_paths),
        "Go" => resolve_go_dependency(import),
        _ => resolve_generic_dependency(file.path.as_path(), import, known_paths),
    }
}

fn resolve_rust_dependency(
    from_path: &Path,
    import: &str,
    known_paths: &[PathBuf],
) -> ResolvedDependency {
    let segments = rust_import_segments(import);
    let Some(first) = segments.first().map(String::as_str) else {
        return unresolved(import);
    };
    if matches!(first, "std" | "core" | "alloc") {
        return standard_library(first);
    }
    if first == "crate" {
        if let Some(path) = resolve_rust_crate_path(&segments[1..], known_paths) {
            return internal(path);
        }
        return unresolved(import);
    }
    if first == "self" || first == "super" {
        if let Some(path) = resolve_rust_relative_path(from_path, &segments, known_paths) {
            return internal(path);
        }
        return unresolved(import);
    }
    if let Some(path) = resolve_rust_sibling_module(from_path, &segments, known_paths) {
        return internal(path);
    }
    external(first)
}

fn rust_import_segments(import: &str) -> Vec<String> {
    import
        .split("::")
        .map(|segment| {
            segment
                .trim()
                .trim_matches(|character: char| {
                    matches!(character, ';' | '{' | '}' | '(' | ')' | ',' | ' ')
                })
                .to_string()
        })
        .filter(|segment| {
            !segment.is_empty()
                && segment != "*"
                && !segment.contains(',')
                && !segment.starts_with('{')
                && !segment.ends_with('}')
        })
        .collect()
}

fn resolve_rust_crate_path(segments: &[String], known_paths: &[PathBuf]) -> Option<String> {
    resolve_module_segments(
        Path::new("src"),
        segments,
        rust_candidate_suffixes(),
        known_paths,
    )
}

fn resolve_rust_relative_path(
    from_path: &Path,
    segments: &[String],
    known_paths: &[PathBuf],
) -> Option<String> {
    let mut base = rust_module_base_dir(from_path);
    let mut remaining = segments;
    while let Some(first) = remaining.first().map(String::as_str) {
        match first {
            "self" => remaining = &remaining[1..],
            "super" => {
                base.pop();
                remaining = &remaining[1..];
            }
            _ => break,
        }
    }
    resolve_module_segments(&base, remaining, rust_candidate_suffixes(), known_paths)
}

fn resolve_rust_sibling_module(
    from_path: &Path,
    segments: &[String],
    known_paths: &[PathBuf],
) -> Option<String> {
    resolve_module_segments(
        &rust_module_base_dir(from_path),
        segments,
        rust_candidate_suffixes(),
        known_paths,
    )
}

fn rust_module_base_dir(from_path: &Path) -> PathBuf {
    from_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn rust_candidate_suffixes() -> &'static [&'static str] {
    &["rs", "mod.rs"]
}

fn resolve_js_dependency(
    from_path: &Path,
    import: &str,
    known_paths: &[PathBuf],
) -> ResolvedDependency {
    if import.starts_with('.') {
        let base = from_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if let Some(path) =
            resolve_relative_import(&base, import, js_candidate_suffixes(), known_paths)
        {
            return internal(path);
        }
        return unresolved(import);
    }
    let package = package_root(import);
    if is_node_builtin(package) {
        return standard_library(package.strip_prefix("node:").unwrap_or(package));
    }
    external(package)
}

fn js_candidate_suffixes() -> &'static [&'static str] {
    &[
        "ts",
        "tsx",
        "js",
        "jsx",
        "mjs",
        "cjs",
        "json",
        "index.ts",
        "index.tsx",
        "index.js",
        "index.jsx",
    ]
}

fn resolve_python_dependency(
    from_path: &Path,
    import: &str,
    known_paths: &[PathBuf],
) -> ResolvedDependency {
    if import.starts_with('.') {
        let base = from_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let stripped = import.trim_start_matches('.');
        if let Some(path) = resolve_module_segments(
            &base,
            &dotted_segments(stripped),
            python_candidate_suffixes(),
            known_paths,
        ) {
            return internal(path);
        }
        return unresolved(import);
    }
    let segments = dotted_segments(import);
    if let Some(path) = resolve_module_segments(
        Path::new(""),
        &segments,
        python_candidate_suffixes(),
        known_paths,
    ) {
        return internal(path);
    }
    let root = segments.first().map(String::as_str).unwrap_or(import);
    if is_python_standard_library(root) {
        return standard_library(root);
    }
    external(root)
}

fn python_candidate_suffixes() -> &'static [&'static str] {
    &["py", "__init__.py"]
}

fn resolve_go_dependency(import: &str) -> ResolvedDependency {
    if is_go_standard_library(import) {
        standard_library(import)
    } else {
        external(import)
    }
}

fn resolve_generic_dependency(
    from_path: &Path,
    import: &str,
    known_paths: &[PathBuf],
) -> ResolvedDependency {
    let base = from_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    if import.starts_with('.') {
        if let Some(path) =
            resolve_relative_import(&base, import, js_candidate_suffixes(), known_paths)
        {
            return internal(path);
        }
        unresolved(import)
    } else {
        external(package_root(import))
    }
}

fn resolve_module_segments(
    base: &Path,
    segments: &[String],
    suffixes: &[&str],
    known_paths: &[PathBuf],
) -> Option<String> {
    for end in (1..=segments.len()).rev() {
        let mut candidate = base.to_path_buf();
        for segment in &segments[..end] {
            candidate.push(segment);
        }
        if let Some(path) = resolve_candidate_path(&candidate, suffixes, known_paths) {
            return Some(path);
        }
    }
    None
}

fn resolve_relative_import(
    base: &Path,
    import: &str,
    suffixes: &[&str],
    known_paths: &[PathBuf],
) -> Option<String> {
    let candidate = normalize_relative_path(base.join(import));
    resolve_candidate_path(&candidate, suffixes, known_paths)
}

fn resolve_candidate_path(
    candidate: &Path,
    suffixes: &[&str],
    known_paths: &[PathBuf],
) -> Option<String> {
    if known_paths.iter().any(|path| path == candidate) {
        return Some(candidate.display().to_string());
    }
    for suffix in suffixes {
        let path = if matches!(*suffix, "mod.rs" | "__init__.py") || suffix.starts_with("index.") {
            candidate.join(suffix)
        } else {
            candidate.with_extension(suffix)
        };
        if known_paths.iter().any(|known| known == &path) {
            return Some(path.display().to_string());
        }
    }
    None
}

fn normalize_relative_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn dotted_segments(value: &str) -> Vec<String> {
    value
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn package_root(import: &str) -> &str {
    if import.starts_with('@') {
        let mut parts = import.split('/');
        let scope = parts.next().unwrap_or(import);
        let package = parts.next().unwrap_or_default();
        if package.is_empty() {
            scope
        } else {
            let end = scope.len() + 1 + package.len();
            &import[..end]
        }
    } else {
        import.split('/').next().unwrap_or(import)
    }
}

fn is_test_file(path: &Path) -> bool {
    let value = path.to_string_lossy().to_lowercase();
    value.contains("/test")
        || value.contains("/tests/")
        || value.contains("__tests__")
        || value.ends_with("_test.go")
        || value.ends_with("_test.rs")
        || value.ends_with(".test.ts")
        || value.ends_with(".test.tsx")
        || value.ends_with(".spec.ts")
        || value.ends_with(".spec.tsx")
        || value.ends_with("_test.py")
        || value.starts_with("tests/")
}

fn is_node_builtin(value: &str) -> bool {
    let value = value.strip_prefix("node:").unwrap_or(value);
    matches!(
        value,
        "assert"
            | "buffer"
            | "child_process"
            | "crypto"
            | "events"
            | "fs"
            | "http"
            | "https"
            | "net"
            | "os"
            | "path"
            | "process"
            | "stream"
            | "url"
            | "util"
    )
}

fn is_python_standard_library(value: &str) -> bool {
    matches!(
        value,
        "asyncio"
            | "collections"
            | "dataclasses"
            | "functools"
            | "json"
            | "logging"
            | "os"
            | "pathlib"
            | "re"
            | "sys"
            | "time"
            | "typing"
            | "unittest"
    )
}

fn is_go_standard_library(value: &str) -> bool {
    !value.contains('.') && !value.starts_with("./") && !value.starts_with("../")
}

fn internal(path: String) -> ResolvedDependency {
    ResolvedDependency {
        to: path,
        target_type: "internal".to_string(),
    }
}

fn external(name: &str) -> ResolvedDependency {
    ResolvedDependency {
        to: name.to_string(),
        target_type: "external".to_string(),
    }
}

fn standard_library(name: &str) -> ResolvedDependency {
    ResolvedDependency {
        to: name.to_string(),
        target_type: "standard-library".to_string(),
    }
}

fn unresolved(import: &str) -> ResolvedDependency {
    ResolvedDependency {
        to: import.to_string(),
        target_type: "unresolved".to_string(),
    }
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
        assert_eq!(structure.dependencies[0].to, "std");
        assert_eq!(structure.dependencies[0].target_type, "standard-library");
        assert_eq!(structure.dependencies[0].evidence, "std::fs");
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
        assert_eq!(report.structure.dependencies[0].to, "std");
        assert_eq!(
            report.structure.dependencies[0].target_type,
            "standard-library"
        );
        assert_eq!(report.structure.dependencies[0].evidence, "std::fs");
    }

    #[test]
    fn classifies_internal_external_standard_and_test_dependencies() {
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
                        longest_line: 40,
                    },
                    content: "use crate::scanner::scan_path;\nuse serde::Serialize;\nuse std::fs;\nmod config;\n"
                        .to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("src/scanner.rs"),
                    language: "Rust".to_string(),
                    bytes: 20,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 1,
                        code_lines: 1,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 12,
                    },
                    content: "fn scan_path() {}\n".to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("src/config.rs"),
                    language: "Rust".to_string(),
                    bytes: 20,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 1,
                        code_lines: 1,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 12,
                    },
                    content: "struct Config;\n".to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("src/app.test.ts"),
                    language: "TypeScript".to_string(),
                    bytes: 80,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 3,
                        code_lines: 3,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 32,
                    },
                    content:
                        "import { run } from './lib';\nimport { test } from 'vitest';\nimport fs from 'node:fs';\n"
                            .to_string(),
                },
                ScannedFile {
                    path: PathBuf::from("src/lib.ts"),
                    language: "TypeScript".to_string(),
                    bytes: 20,
                    truncated: false,
                    metrics: FileMetrics {
                        lines: 1,
                        code_lines: 1,
                        comment_lines: 0,
                        blank_lines: 0,
                        longest_line: 12,
                    },
                    content: "export function run() {}\n".to_string(),
                },
            ],
            skipped: vec![],
            summary: ScanSummary::default(),
        };

        let structure = infer_structure(&snapshot);

        assert!(structure
            .dependencies
            .iter()
            .any(|dependency| dependency.from == "src/lib.rs"
                && dependency.to == "src/scanner.rs"
                && dependency.kind == "import"
                && dependency.target_type == "internal"
                && dependency.evidence == "crate::scanner::scan_path"));
        assert!(structure
            .dependencies
            .iter()
            .any(|dependency| dependency.from == "src/lib.rs"
                && dependency.to == "src/config.rs"
                && dependency.target_type == "internal"
                && dependency.evidence == "config"));
        assert!(structure
            .dependencies
            .iter()
            .any(|dependency| dependency.from == "src/lib.rs"
                && dependency.to == "serde"
                && dependency.target_type == "external"));
        assert!(structure
            .dependencies
            .iter()
            .any(|dependency| dependency.from == "src/lib.rs"
                && dependency.to == "std"
                && dependency.target_type == "standard-library"));
        assert!(structure.dependencies.iter().any(|dependency| {
            dependency.from == "src/app.test.ts"
                && dependency.to == "src/lib.ts"
                && dependency.kind == "test"
                && dependency.target_type == "internal"
                && dependency.evidence == "./lib"
        }));
        assert!(structure.dependencies.iter().any(|dependency| {
            dependency.from == "src/app.test.ts"
                && dependency.to == "vitest"
                && dependency.kind == "test"
                && dependency.target_type == "external"
        }));
        assert!(structure.dependencies.iter().any(|dependency| {
            dependency.from == "src/app.test.ts"
                && dependency.to == "fs"
                && dependency.kind == "test"
                && dependency.target_type == "standard-library"
        }));
    }
}
