use anyhow::Result;
use globset::{Glob, GlobSetBuilder};
use ignore::WalkBuilder;
use std::path::Path;

/// Discover all files under `root`, stopping at child scope boundaries
/// (directories containing `drft.lock` or `drft.toml`). Respects `.gitignore` automatically.
/// Returns paths relative to `root`, sorted.
pub fn discover(root: &Path, ignore_patterns: &[String]) -> Result<Vec<String>> {
    let ignore_set = if ignore_patterns.is_empty() {
        None
    } else {
        let mut builder = GlobSetBuilder::new();
        for pattern in ignore_patterns {
            builder.add(Glob::new(pattern)?);
        }
        Some(builder.build()?)
    };

    let mut files = Vec::new();
    let root_owned = root.to_path_buf();

    let walker = WalkBuilder::new(root)
        .follow_links(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |entry| {
            if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                if entry.path() == root_owned {
                    return true;
                }
                // Stop at child scope boundaries
                if entry.path().join("drft.lock").exists()
                    || entry.path().join("drft.toml").exists()
                {
                    return false;
                }
            }
            true
        })
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

        if let Some(ref set) = ignore_set
            && set.is_match(&relative)
        {
            continue;
        }

        files.push(relative);
    }

    files.sort();
    Ok(files)
}

/// Find child scope directories (those containing `drft.lock` or `drft.toml`) under `root`.
/// Returns relative paths with trailing slash (e.g., `"research/"`), sorted.
/// Only returns the shallowest boundary — does not recurse past them.
/// Respects `.gitignore` and `ignore_patterns` from config.
pub fn find_child_scopes(root: &Path, ignore_patterns: &[String]) -> Result<Vec<String>> {
    let ignore_set = if ignore_patterns.is_empty() {
        None
    } else {
        let mut builder = GlobSetBuilder::new();
        for pattern in ignore_patterns {
            builder.add(Glob::new(pattern)?);
        }
        Some(builder.build()?)
    };

    let mut scopes = Vec::new();
    let root_owned = root.to_path_buf();

    // Use the ignore crate to respect .gitignore, and stop recursing
    // past child scope boundaries.
    let walker = WalkBuilder::new(root)
        .follow_links(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |entry| {
            if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
                return false; // skip files, we only care about directories
            }
            if entry.path() == root_owned {
                return true;
            }
            // Allow entry so we can inspect it, but we'll track boundaries below
            true
        })
        .build();

    let mut found_prefixes: Vec<String> = Vec::new();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        if entry.path() == root {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .expect("path should be under root")
            .to_string_lossy()
            .replace('\\', "/");

        // Skip if inside an already-found child scope
        let inside_existing = found_prefixes
            .iter()
            .any(|s| relative.starts_with(s.as_str()));
        if inside_existing {
            continue;
        }

        if entry.path().join("drft.lock").exists() || entry.path().join("drft.toml").exists() {
            // Skip if matched by ignore patterns
            if let Some(ref set) = ignore_set
                && set.is_match(&relative)
            {
                continue;
            }

            let scope_path = format!("{relative}/");
            found_prefixes.push(scope_path.clone());
            scopes.push(scope_path);
        }
    }

    scopes.sort();
    Ok(scopes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discovers_all_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
        fs::write(dir.path().join("notes.txt"), "not markdown").unwrap();

        let files = discover(dir.path(), &[]).unwrap();
        assert_eq!(files, vec!["index.md", "notes.txt", "setup.md"]);
    }

    #[test]
    fn stops_at_scope_boundary() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Root").unwrap();

        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("drft.lock"), "").unwrap();
        fs::write(child.join("inner.md"), "# Inner").unwrap();

        let files = discover(dir.path(), &[]).unwrap();
        assert_eq!(files, vec!["index.md"]);
    }

    #[test]
    fn respects_ignore_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        let drafts = dir.path().join("drafts");
        fs::create_dir(&drafts).unwrap();
        fs::write(drafts.join("wip.md"), "# WIP").unwrap();

        let files = discover(dir.path(), &["drafts/*".to_string()]).unwrap();
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

        let files = discover(dir.path(), &[]).unwrap();
        assert_eq!(files, vec!["index.md"]);
    }

    #[test]
    fn finds_child_scopes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Root").unwrap();

        let alpha = dir.path().join("alpha");
        fs::create_dir(&alpha).unwrap();
        fs::write(alpha.join("drft.lock"), "").unwrap();

        let beta = dir.path().join("beta");
        fs::create_dir(&beta).unwrap();
        fs::write(beta.join("drft.lock"), "").unwrap();

        // No lockfile in gamma — not a child scope
        let gamma = dir.path().join("gamma");
        fs::create_dir(&gamma).unwrap();
        fs::write(gamma.join("readme.md"), "").unwrap();

        let scopes = find_child_scopes(dir.path(), &[]).unwrap();
        assert_eq!(scopes, vec!["alpha/", "beta/"]);
    }

    #[test]
    fn child_scopes_stops_at_boundary() {
        let dir = TempDir::new().unwrap();
        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("drft.lock"), "").unwrap();

        // Grandchild scope — should NOT appear from parent's perspective
        let grandchild = child.join("nested");
        fs::create_dir(&grandchild).unwrap();
        fs::write(grandchild.join("drft.lock"), "").unwrap();

        let scopes = find_child_scopes(dir.path(), &[]).unwrap();
        assert_eq!(scopes, vec!["child/"]);
    }
}
