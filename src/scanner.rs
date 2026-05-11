use anyhow::{Context, Result};
use serde::Serialize;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".turbo",
    "coverage",
];

const IGNORED_FILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lockb",
    "go.sum",
    "composer.lock",
    "Gemfile.lock",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub root: PathBuf,
    pub files: Vec<ScannedFile>,
    pub skipped: Vec<SkippedFile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub language: String,
    pub bytes: u64,
    pub truncated: bool,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub max_file_bytes: u64,
}

pub fn scan_path(path: impl AsRef<Path>, options: ScanOptions) -> Result<ProjectSnapshot> {
    let path = path.as_ref();
    let root = path
        .canonicalize()
        .with_context(|| format!("failed to access {}", path.display()))?;

    if root.is_file() {
        return scan_file_root(root, options);
    }

    let mut snapshot = ProjectSnapshot {
        root: root.clone(),
        files: Vec::new(),
        skipped: Vec::new(),
    };

    for entry in WalkDir::new(&root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                snapshot.skipped.push(SkippedFile {
                    path: error
                        .path()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| root.clone()),
                    reason: error.to_string(),
                });
                continue;
            }
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        if is_ignored_file(file_path) {
            snapshot.skipped.push(SkippedFile {
                path: relative_path(&root, file_path),
                reason: "ignored lock or generated file".to_string(),
            });
            continue;
        }

        match scan_file(&root, file_path, options) {
            Ok(file) => snapshot.files.push(file),
            Err(error) => snapshot.skipped.push(SkippedFile {
                path: relative_path(&root, file_path),
                reason: error.to_string(),
            }),
        }
    }

    snapshot
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    snapshot
        .skipped
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(snapshot)
}

fn scan_file_root(path: PathBuf, options: ScanOptions) -> Result<ProjectSnapshot> {
    let root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut snapshot = ProjectSnapshot {
        root: root.clone(),
        files: Vec::new(),
        skipped: Vec::new(),
    };

    if is_ignored_file(&path) {
        snapshot.skipped.push(SkippedFile {
            path: relative_path(&root, &path),
            reason: "ignored lock or generated file".to_string(),
        });
        return Ok(snapshot);
    }

    match scan_file(&root, &path, options) {
        Ok(file) => snapshot.files.push(file),
        Err(error) => snapshot.skipped.push(SkippedFile {
            path: relative_path(&root, &path),
            reason: error.to_string(),
        }),
    }

    Ok(snapshot)
}

fn scan_file(root: &Path, path: &Path, options: ScanOptions) -> Result<ScannedFile> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?;
    let bytes = metadata.len();
    let read_limit = options.max_file_bytes.saturating_add(1) as usize;
    let mut content = fs::read_to_string(path)
        .with_context(|| format!("failed to read text file {}", path.display()))?;
    let truncated = content.len() > read_limit;
    if truncated {
        content.truncate(options.max_file_bytes as usize);
        content.push_str("\n\n[deepcode: file content truncated]\n");
    }

    Ok(ScannedFile {
        path: relative_path(root, path),
        language: language_for_path(path).to_string(),
        bytes,
        truncated,
        content,
    })
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| IGNORED_DIRS.contains(&name))
}

fn is_ignored_file(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| IGNORED_FILES.contains(&name))
}

fn relative_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn language_for_path(path: &Path) -> &'static str {
    match path.extension().and_then(OsStr::to_str).unwrap_or_default() {
        "rs" => "Rust",
        "go" => "Go",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "jsx" => "JavaScript JSX",
        "py" => "Python",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "cs" => "C#",
        "php" => "PHP",
        "rb" => "Ruby",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "html" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "Sass",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "md" | "mdx" => "Markdown",
        "sql" => "SQL",
        "sh" | "bash" | "zsh" => "Shell",
        _ => "Text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_files_and_ignores_common_generated_paths() {
        let dir = temp_dir("scan");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join("target").join("debug.txt"), "generated\n").unwrap();
        fs::write(dir.join("Cargo.lock"), "lock\n").unwrap();

        let snapshot = scan_path(
            &dir,
            ScanOptions {
                max_file_bytes: 100,
            },
        )
        .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(snapshot.files[0].language, "Rust");
        assert_eq!(snapshot.skipped.len(), 1);
        assert_eq!(snapshot.skipped[0].path, PathBuf::from("Cargo.lock"));
    }

    #[test]
    fn truncates_large_text_files() {
        let dir = temp_dir("truncate");
        let file = dir.join("big.txt");
        fs::write(&file, "abcdef").unwrap();

        let snapshot = scan_path(&file, ScanOptions { max_file_bytes: 3 }).unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot.files[0].truncated);
        assert!(snapshot.files[0].content.starts_with("abc"));
        assert!(snapshot.files[0].content.contains("truncated"));
    }

    #[test]
    fn records_binary_or_invalid_utf8_as_skipped() {
        let dir = temp_dir("binary");
        fs::write(dir.join("bad.bin"), [0, 159, 146, 150]).unwrap();

        let snapshot = scan_path(
            &dir,
            ScanOptions {
                max_file_bytes: 100,
            },
        )
        .unwrap();

        assert!(snapshot.files.is_empty());
        assert_eq!(snapshot.skipped.len(), 1);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("deepcode-scanner-{name}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
