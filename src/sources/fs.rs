//! The `fs` source: a `.gitignore`-aware filesystem walk that yields one
//! [`SourceFile`] per file under the graph root.

use anyhow::Result;
use ignore::WalkBuilder;
use std::path::Path;

use crate::config::compile_globs;

/// A file delivered by a source: its graph-relative path and, when read, its
/// raw bytes.
pub struct SourceFile {
    /// Path relative to the graph root, with forward slashes.
    pub path: String,
    /// Whether this entry is a symlink (stat'd once during the walk).
    pub is_symlink: bool,
    /// Raw content. `None` when the file exists but its content is intentionally
    /// not read — a symlink whose target resolves outside the graph root, or a
    /// file that could not be read.
    pub bytes: Option<Vec<u8>>,
}

/// Walk the tree under `root`, honoring `.gitignore` and the `ignore` globs,
/// yielding one [`SourceFile`] per file. Paths are relative to `root`, sorted.
///
/// A symlink is yielded at its own path; its content is read only when the link
/// resolves within `root`, so a symlink escaping the graph carries no bytes
/// (and therefore no hash).
pub fn walk(root: &Path, ignore: &[String]) -> Result<Vec<SourceFile>> {
    let ignore_set = compile_globs(ignore)?;
    let canonical_root = root.canonicalize()?;

    let mut files = Vec::new();

    let walker = WalkBuilder::new(root).follow_links(true).build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .expect("path should be under root")
            .to_string_lossy()
            .replace('\\', "/");

        if ignore_set
            .as_ref()
            .is_some_and(|set| set.is_match(&relative))
        {
            continue;
        }

        let abs = root.join(&relative);
        let is_symlink = abs
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());

        let bytes = if is_symlink {
            // Read content only when the link resolves within the graph root.
            match abs.canonicalize() {
                Ok(canonical) if canonical.starts_with(&canonical_root) => std::fs::read(&abs).ok(),
                _ => None,
            }
        } else {
            std::fs::read(&abs).ok()
        };

        files.push(SourceFile {
            path: relative,
            is_symlink,
            bytes,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn walks_all_files_sorted() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("b.md"), "b").unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        fs::write(dir.path().join("notes.txt"), "n").unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md", "notes.txt"]);
        assert_eq!(files[0].bytes.as_deref(), Some(&b"a"[..]));
    }

    #[test]
    fn respects_ignore_globs() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("keep.md"), "k").unwrap();
        let sub = dir.path().join("target");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("build.md"), "b").unwrap();

        let files = walk(dir.path(), &["target/**".to_string()]).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["keep.md"]);
    }

    #[test]
    fn respects_gitignore() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "vendor/\n").unwrap();
        fs::write(dir.path().join("index.md"), "i").unwrap();
        let vendor = dir.path().join("vendor");
        fs::create_dir(&vendor).unwrap();
        fs::write(vendor.join("lib.md"), "v").unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"index.md"));
        assert!(!paths.iter().any(|p| p.contains("vendor")));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escaping_root_has_no_bytes() {
        let outer = TempDir::new().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("index.md"), "i").unwrap();
        fs::write(outer.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(outer.path().join("secret.md"), root.join("trap.md")).unwrap();

        let files = walk(&root, &[]).unwrap();
        let trap = files.iter().find(|f| f.path == "trap.md").unwrap();
        assert!(
            trap.bytes.is_none(),
            "escaping symlink should carry no bytes"
        );
    }
}
