use anyhow::{Context, Result};
use serde::Serialize;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::thread;
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
    pub summary: ScanSummary,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub language: String,
    pub bytes: u64,
    pub truncated: bool,
    pub metrics: FileMetrics,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileMetrics {
    pub lines: usize,
    pub code_lines: usize,
    pub comment_lines: usize,
    pub blank_lines: usize,
    pub longest_line: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct ScanSummary {
    pub files_read: usize,
    pub files_skipped: usize,
    pub bytes_read: u64,
    pub total_lines: usize,
    pub total_code_lines: usize,
    pub languages: Vec<LanguageSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LanguageSummary {
    pub language: String,
    pub files: usize,
    pub bytes: u64,
    pub code_lines: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub max_file_bytes: u64,
    pub max_files: usize,
    pub max_total_bytes: u64,
    pub max_concurrency: usize,
}

#[derive(Debug, Clone)]
struct CandidateFile {
    root: PathBuf,
    path: PathBuf,
    bytes: u64,
}

pub fn scan_path(path: impl AsRef<Path>, options: ScanOptions) -> Result<ProjectSnapshot> {
    let path = path.as_ref();
    let root = path
        .canonicalize()
        .with_context(|| format!("failed to access {}", path.display()))?;

    if root.is_file() {
        return scan_file_root(root, options);
    }

    let mut skipped = Vec::new();
    let mut candidates = Vec::new();
    let mut scheduled_bytes = 0_u64;

    for entry in WalkDir::new(&root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                skipped.push(SkippedFile {
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
            skipped.push(SkippedFile {
                path: relative_path(&root, file_path),
                reason: "ignored lock or generated file".to_string(),
            });
            continue;
        }

        if candidates.len() >= options.max_files {
            skipped.push(SkippedFile {
                path: relative_path(&root, file_path),
                reason: format!("scan file limit reached ({})", options.max_files),
            });
            continue;
        }

        let bytes = match fs::metadata(file_path) {
            Ok(metadata) => metadata.len(),
            Err(error) => {
                skipped.push(SkippedFile {
                    path: relative_path(&root, file_path),
                    reason: format!("failed to read metadata: {error}"),
                });
                continue;
            }
        };
        if scheduled_bytes.saturating_add(bytes) > options.max_total_bytes {
            skipped.push(SkippedFile {
                path: relative_path(&root, file_path),
                reason: format!("scan byte budget exceeded ({})", options.max_total_bytes),
            });
            continue;
        }
        scheduled_bytes += bytes;
        candidates.push(CandidateFile {
            root: root.clone(),
            path: file_path.to_path_buf(),
            bytes,
        });
    }

    let mut files = Vec::new();
    for result in read_candidates(candidates, options) {
        match result {
            Ok(file) => files.push(file),
            Err((path, error)) => skipped.push(SkippedFile {
                path,
                reason: error.to_string(),
            }),
        }
    }

    let mut snapshot = ProjectSnapshot {
        root,
        files,
        skipped,
        summary: ScanSummary::default(),
    };
    snapshot
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    snapshot
        .skipped
        .sort_by(|left, right| left.path.cmp(&right.path));
    snapshot.summary = summarize(&snapshot.files, &snapshot.skipped);
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
        summary: ScanSummary::default(),
    };

    if is_ignored_file(&path) {
        snapshot.skipped.push(SkippedFile {
            path: relative_path(&root, &path),
            reason: "ignored lock or generated file".to_string(),
        });
        snapshot.summary = summarize(&snapshot.files, &snapshot.skipped);
        return Ok(snapshot);
    }

    match scan_file(&root, &path, options, None) {
        Ok(file) => snapshot.files.push(file),
        Err(error) => snapshot.skipped.push(SkippedFile {
            path: relative_path(&root, &path),
            reason: error.to_string(),
        }),
    }

    snapshot.summary = summarize(&snapshot.files, &snapshot.skipped);
    Ok(snapshot)
}

fn read_candidates(
    candidates: Vec<CandidateFile>,
    options: ScanOptions,
) -> Vec<Result<ScannedFile, (PathBuf, anyhow::Error)>> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let chunk_size = options.max_concurrency.max(1);
    let mut output = Vec::with_capacity(candidates.len());
    for chunk in candidates.chunks(chunk_size) {
        let mut handles = Vec::with_capacity(chunk.len());
        for candidate in chunk.iter().cloned() {
            handles.push(thread::spawn(move || {
                let relative = relative_path(&candidate.root, &candidate.path);
                scan_file(
                    &candidate.root,
                    &candidate.path,
                    options,
                    Some(candidate.bytes),
                )
                .map_err(|error| (relative, error))
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(result) => output.push(result),
                Err(_) => output.push(Err((
                    PathBuf::from("<thread>"),
                    anyhow::anyhow!("scan worker thread panicked"),
                ))),
            }
        }
    }
    output
}

fn scan_file(
    root: &Path,
    path: &Path,
    options: ScanOptions,
    known_bytes: Option<u64>,
) -> Result<ScannedFile> {
    let bytes = match known_bytes {
        Some(bytes) => bytes,
        None => fs::metadata(path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?
            .len(),
    };
    let read_limit = options.max_file_bytes.saturating_add(1) as usize;
    let mut handle =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes_buffer = Vec::with_capacity(read_limit.min(64 * 1024));
    handle
        .by_ref()
        .take(read_limit as u64)
        .read_to_end(&mut bytes_buffer)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let truncated =
        bytes_buffer.len() > options.max_file_bytes as usize || bytes > options.max_file_bytes;
    if bytes_buffer.len() > options.max_file_bytes as usize {
        truncate_to_char_boundary(&mut bytes_buffer, options.max_file_bytes as usize);
    }
    let mut content = String::from_utf8(bytes_buffer)
        .with_context(|| format!("failed to read text file {}", path.display()))?;
    if truncated {
        content.push_str("\n\n[deepcode: file content truncated]\n");
    }
    let metrics = measure_content(&content);

    Ok(ScannedFile {
        path: relative_path(root, path),
        language: language_for_path(path).to_string(),
        bytes,
        truncated,
        metrics,
        content,
    })
}

fn truncate_to_char_boundary(bytes: &mut Vec<u8>, max_len: usize) {
    bytes.truncate(max_len);
    while std::str::from_utf8(bytes).is_err() && !bytes.is_empty() {
        bytes.pop();
    }
}

fn summarize(files: &[ScannedFile], skipped: &[SkippedFile]) -> ScanSummary {
    let mut languages: Vec<LanguageSummary> = Vec::new();
    for file in files {
        let entry = match languages
            .iter_mut()
            .find(|summary| summary.language == file.language)
        {
            Some(entry) => entry,
            None => {
                languages.push(LanguageSummary {
                    language: file.language.clone(),
                    files: 0,
                    bytes: 0,
                    code_lines: 0,
                });
                languages.last_mut().expect("language summary exists")
            }
        };
        entry.files += 1;
        entry.bytes += file.bytes;
        entry.code_lines += file.metrics.code_lines;
    }
    languages.sort_by(|left, right| {
        right
            .code_lines
            .cmp(&left.code_lines)
            .then_with(|| left.language.cmp(&right.language))
    });

    ScanSummary {
        files_read: files.len(),
        files_skipped: skipped.len(),
        bytes_read: files.iter().map(|file| file.bytes).sum(),
        total_lines: files.iter().map(|file| file.metrics.lines).sum(),
        total_code_lines: files.iter().map(|file| file.metrics.code_lines).sum(),
        languages,
    }
}

fn measure_content(content: &str) -> FileMetrics {
    let mut metrics = FileMetrics {
        lines: 0,
        code_lines: 0,
        comment_lines: 0,
        blank_lines: 0,
        longest_line: 0,
    };
    for line in content.lines() {
        metrics.lines += 1;
        metrics.longest_line = metrics.longest_line.max(line.len());
        let trimmed = line.trim();
        if trimmed.is_empty() {
            metrics.blank_lines += 1;
        } else if is_comment_line(trimmed) {
            metrics.comment_lines += 1;
        } else {
            metrics.code_lines += 1;
        }
    }
    metrics
}

fn is_comment_line(trimmed: &str) -> bool {
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("--")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("*/")
        || trimmed.starts_with("<!--")
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
                max_files: 100,
                max_total_bytes: 1_000,
                max_concurrency: 4,
            },
        )
        .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.files[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(snapshot.files[0].language, "Rust");
        assert_eq!(snapshot.files[0].metrics.code_lines, 1);
        assert_eq!(snapshot.summary.files_read, 1);
        assert_eq!(snapshot.summary.languages[0].language, "Rust");
        assert_eq!(snapshot.skipped.len(), 1);
        assert_eq!(snapshot.skipped[0].path, PathBuf::from("Cargo.lock"));
    }

    #[test]
    fn truncates_large_text_files() {
        let dir = temp_dir("truncate");
        let file = dir.join("big.txt");
        fs::write(&file, "abcdef").unwrap();

        let snapshot = scan_path(
            &file,
            ScanOptions {
                max_file_bytes: 3,
                max_files: 100,
                max_total_bytes: 1_000,
                max_concurrency: 4,
            },
        )
        .unwrap();

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
                max_files: 100,
                max_total_bytes: 1_000,
                max_concurrency: 4,
            },
        )
        .unwrap();

        assert!(snapshot.files.is_empty());
        assert_eq!(snapshot.skipped.len(), 1);
    }

    #[test]
    fn enforces_file_budget() {
        let dir = temp_dir("file-budget");
        fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();

        let snapshot = scan_path(
            &dir,
            ScanOptions {
                max_file_bytes: 100,
                max_files: 1,
                max_total_bytes: 1_000,
                max_concurrency: 4,
            },
        )
        .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.skipped.len(), 1);
        assert!(snapshot.skipped[0].reason.contains("scan file limit"));
    }

    #[test]
    fn enforces_total_byte_budget() {
        let dir = temp_dir("byte-budget");
        fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();

        let snapshot = scan_path(
            &dir,
            ScanOptions {
                max_file_bytes: 100,
                max_files: 100,
                max_total_bytes: 10,
                max_concurrency: 4,
            },
        )
        .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert_eq!(snapshot.skipped.len(), 1);
        assert!(snapshot.skipped[0].reason.contains("scan byte budget"));
    }

    #[test]
    fn scans_with_bounded_parallelism_and_stable_order() {
        let dir = temp_dir("parallel");
        fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.join("c.rs"), "fn c() {}\n").unwrap();

        let snapshot = scan_path(
            &dir,
            ScanOptions {
                max_file_bytes: 100,
                max_files: 100,
                max_total_bytes: 1_000,
                max_concurrency: 2,
            },
        )
        .unwrap();

        let paths = snapshot
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("a.rs"),
                PathBuf::from("b.rs"),
                PathBuf::from("c.rs")
            ]
        );
    }

    #[test]
    fn truncates_utf8_at_valid_boundary() {
        let dir = temp_dir("utf8");
        let file = dir.join("utf8.txt");
        fs::write(&file, "你好abcdef").unwrap();

        let snapshot = scan_path(
            &file,
            ScanOptions {
                max_file_bytes: 4,
                max_files: 100,
                max_total_bytes: 1_000,
                max_concurrency: 2,
            },
        )
        .unwrap();

        assert_eq!(snapshot.files.len(), 1);
        assert!(snapshot.files[0].truncated);
        assert!(snapshot.files[0].content.starts_with("你"));
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
