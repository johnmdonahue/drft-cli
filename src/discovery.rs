use anyhow::Result;
use globset::GlobSet;
use ignore::WalkBuilder;
use std::path::Path;

use crate::config::compile_globs;

/// Discover files under `root` matching `include` patterns (minus `exclude`).
/// Respects `.gitignore` automatically. Returns paths relative to `root`, sorted.
pub fn discover(
    root: &Path,
    include_patterns: &[String],
    exclude_patterns: &[String],
) -> Result<Vec<String>> {
    let include_set = compile_globs(include_patterns)?.unwrap_or_else(GlobSet::empty);
    let exclude_set = compile_globs(exclude_patterns)?;

    let mut files = Vec::new();

    let walker = WalkBuilder::new(root)
        .follow_links(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();

        let relative = path
            .strip_prefix(root)
            .expect("path should be under root")
            .to_string_lossy()
            .replace('\\', "/");

        // Must match at least one include pattern
        if !include_set.is_match(&relative) {
            continue;
        }

        // Must not match any exclude pattern
        if let Some(ref set) = exclude_set
            && set.is_match(&relative)
        {
            continue;
        }

        files.push(relative);
    }

    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discovers_files_matching_include() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
        fs::write(dir.path().join("notes.txt"), "not markdown").unwrap();

        // Only .md files
        let files = discover(dir.path(), &["*.md".to_string()], &[]).unwrap();
        assert_eq!(files, vec!["index.md", "setup.md"]);

        // All files
        let files = discover(dir.path(), &["*".to_string()], &[]).unwrap();
        assert_eq!(files, vec!["index.md", "notes.txt", "setup.md"]);
    }

    #[test]
    fn discovers_through_nested_drft_toml() {
        // Nested drft.toml files are ordinary files — discovery walks through them.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Root").unwrap();

        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("drft.toml"), "").unwrap();
        fs::write(child.join("inner.md"), "# Inner").unwrap();

        let files = discover(dir.path(), &["**/*.md".to_string()], &[]).unwrap();
        assert_eq!(files, vec!["child/inner.md", "index.md"]);
    }

    #[test]
    fn respects_exclude_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        let drafts = dir.path().join("drafts");
        fs::create_dir(&drafts).unwrap();
        fs::write(drafts.join("wip.md"), "# WIP").unwrap();

        let files = discover(dir.path(), &["*.md".to_string()], &["drafts/*".to_string()]).unwrap();
        assert_eq!(files, vec!["index.md"]);
    }

    #[test]
    fn respects_gitignore() {
        let dir = TempDir::new().unwrap();
        // The ignore crate requires a .git dir to activate .gitignore
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "vendor/\n").unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        let vendor = dir.path().join("vendor");
        fs::create_dir(&vendor).unwrap();
        fs::write(vendor.join("lib.md"), "# Vendored").unwrap();

        let files = discover(dir.path(), &["*.md".to_string()], &[]).unwrap();
        assert_eq!(files, vec!["index.md"]);
    }

    #[test]
    fn multiple_include_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        fs::write(dir.path().join("config.yaml"), "key: val").unwrap();
        fs::write(dir.path().join("notes.txt"), "text").unwrap();

        let files = discover(dir.path(), &["*.md".to_string(), "*.yaml".to_string()], &[]).unwrap();
        assert_eq!(files, vec!["config.yaml", "index.md"]);
    }
}
