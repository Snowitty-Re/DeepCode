use crate::report::DiffSummary;
use crate::scanner::ProjectSnapshot;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub fn summarize_diff(
    old_snapshot: &ProjectSnapshot,
    new_snapshot: &ProjectSnapshot,
) -> DiffSummary {
    let old_files = content_hashes(old_snapshot, "old/");
    let new_files = content_hashes(new_snapshot, "new/");
    let old_paths = old_files.keys().cloned().collect::<BTreeSet<_>>();
    let new_paths = new_files.keys().cloned().collect::<BTreeSet<_>>();

    let added = new_paths
        .difference(&old_paths)
        .cloned()
        .collect::<Vec<_>>();
    let removed = old_paths
        .difference(&new_paths)
        .cloned()
        .collect::<Vec<_>>();
    let mut modified = Vec::new();
    let mut unchanged = Vec::new();
    for path in old_paths.intersection(&new_paths) {
        if old_files.get(path) == new_files.get(path) {
            unchanged.push(path.clone());
        } else {
            modified.push(path.clone());
        }
    }

    DiffSummary {
        added,
        removed,
        modified,
        unchanged,
    }
}

fn content_hashes(snapshot: &ProjectSnapshot, prefix: &str) -> BTreeMap<String, String> {
    snapshot
        .files
        .iter()
        .map(|file| {
            let path = file.path.to_string_lossy();
            let normalized = path.strip_prefix(prefix).unwrap_or(&path);
            (normalized.to_string(), hash(&file.content))
        })
        .collect()
}

fn hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::{FileMetrics, ScanSummary, ScannedFile};
    use std::path::PathBuf;

    #[test]
    fn detects_added_removed_modified_and_unchanged_files() {
        let old_snapshot = snapshot(vec![
            file("same.rs", "same"),
            file("changed.rs", "old"),
            file("removed.rs", "gone"),
        ]);
        let new_snapshot = snapshot(vec![
            file("same.rs", "same"),
            file("changed.rs", "new"),
            file("added.rs", "here"),
        ]);

        let diff = summarize_diff(&old_snapshot, &new_snapshot);

        assert_eq!(diff.added, vec!["added.rs"]);
        assert_eq!(diff.removed, vec!["removed.rs"]);
        assert_eq!(diff.modified, vec!["changed.rs"]);
        assert_eq!(diff.unchanged, vec!["same.rs"]);
    }

    fn snapshot(files: Vec<ScannedFile>) -> ProjectSnapshot {
        ProjectSnapshot {
            root: PathBuf::from("/tmp/app"),
            files,
            skipped: vec![],
            summary: ScanSummary::default(),
        }
    }

    fn file(path: &str, content: &str) -> ScannedFile {
        ScannedFile {
            path: PathBuf::from(path),
            language: "Rust".to_string(),
            bytes: content.len() as u64,
            truncated: false,
            metrics: FileMetrics {
                lines: 1,
                code_lines: 1,
                comment_lines: 0,
                blank_lines: 0,
                longest_line: content.len(),
            },
            content: content.to_string(),
        }
    }
}
