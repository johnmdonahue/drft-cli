//! The `fs` source: a `.gitignore`-aware filesystem walk that yields one
//! [`SourceFile`] per file, symlink, and directory under the graph root.

use anyhow::Result;
use globset::GlobSet;
use ignore::{Walk, WalkBuilder};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::config::compile_globs;

/// VCS metadata entries pruned from the walk. The hidden filter is off so that
/// ordinary dot-directories (`.github/`, `.config/`) join the graph, but a
/// version-control store is internal bookkeeping, never graph content — so it
/// is excluded by name. ripgrep skips these via its hidden filter; drft keeps
/// dot-dirs and names the exclusions instead.
const VCS_DIRS: [&str; 4] = [".git", ".hg", ".svn", ".jj"];

/// What kind of filesystem entry a [`SourceFile`] is. Derived from `lstat`, so a
/// symlink-to-directory is [`Symlink`](NodeKind::Symlink) (indirection wins over
/// target kind), and [`Dir`](NodeKind::Dir) always means a real directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Symlink,
    Dir,
}

/// An entry delivered by a source: its graph-relative path, kind, and — for
/// files — its raw bytes.
pub struct SourceFile {
    /// Path relative to the graph root, with forward slashes.
    pub path: String,
    /// The kind of entry, from `lstat`.
    pub kind: NodeKind,
    /// Raw content. `Some` only for files (and `None` for one that could not be
    /// read). Symlinks and directories are untrackable and always carry `None`.
    pub bytes: Option<Vec<u8>>,
}

/// Ignore sources used by the filesystem walk. Paths are relative to the graph
/// root, so the report is stable across checkouts and does not expose home paths.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct IgnoreSources {
    pub gitignore: IgnoreSource,
    pub git_exclude: IgnoreSource,
    pub git_global: IgnoreSource,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct IgnoreSource {
    pub enabled: bool,
    pub files: Vec<String>,
}

/// Walk the tree under `root`, honoring `.gitignore` and the `ignore` globs,
/// yielding one [`SourceFile`] per file, symlink, and directory. Paths are
/// relative to `root`, sorted.
///
/// Hidden entries are *not* skipped: a dot-directory like `.github/` is part of
/// the graph. The lone exception is VCS metadata ([`VCS_DIRS`]), pruned from
/// traversal — `.git/` would otherwise flood the graph with internal state.
///
/// Repository `.gitignore` rules from the graph root through the Git root prune
/// the walk. Nested `.gitignore` files also apply to their subtrees. The user's
/// global excludes and the per-clone `.git/info/exclude` do not apply.
///
/// The walk does not follow symlinks: a symlink is a leaf node at its own path,
/// never traversed through. Its relationship to its target is carried by the
/// edge the builder emits, not by re-walking the target. So a symlink to a
/// directory does not duplicate that directory's subtree, and a symlink to a
/// path outside the root never pulls outside content into the graph.
///
/// Only files carry bytes (and therefore a hash). Symlinks and directories are
/// untrackable: they resolve link targets but are never hashed or locked.
pub fn walk(root: &Path, ignore: &[String]) -> Result<Vec<SourceFile>> {
    let ignore_set = compile_globs(ignore)?;

    let mut files = Vec::new();

    // Repository `.gitignore` files are shared project state, including files
    // between the graph root and Git root. Machine-local global excludes and
    // `.git/info/exclude` remain disabled so they cannot silently change a
    // shared lockfile between checkouts.
    let walker = filesystem_walker(root);

    for entry in walker {
        let entry = entry?;
        let ft = entry.file_type();
        // Yield files, symlinks, and directories; skip fifos, sockets, and other
        // entries. With symlinks unfollowed, a symlink reports its own type here.
        if !ft.is_some_and(|t| t.is_file() || t.is_dir() || t.is_symlink()) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .expect("path should be under root")
            .to_string_lossy()
            .replace('\\', "/");

        // The root itself is the graph, not a node in it.
        if relative.is_empty() {
            continue;
        }

        // Type from `lstat`, symlink first: a symlink-to-dir is a symlink, and
        // `Dir` always means a real directory.
        let kind = match abs_lstat(root, &relative) {
            Some(m) if m.file_type().is_symlink() => NodeKind::Symlink,
            Some(m) if m.is_dir() => NodeKind::Dir,
            _ => NodeKind::File,
        };

        if is_ignored(&ignore_set, &relative, kind) {
            continue;
        }

        // Only files carry content. A symlink's content is its target's, reached
        // through the edge — the symlink node itself stays untrackable.
        let bytes = match kind {
            NodeKind::File => std::fs::read(root.join(&relative)).ok(),
            NodeKind::Symlink | NodeKind::Dir => None,
        };

        files.push(SourceFile {
            path: relative,
            kind,
            bytes,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn filesystem_walker(root: &Path) -> Walk {
    WalkBuilder::new(root)
        .follow_links(false)
        .hidden(false)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(|entry| {
            // With the hidden filter off, dot-directories are walked. Prune VCS
            // metadata explicitly so it never enters the graph. `.git` can be a
            // file (submodules, linked worktrees) as well as a directory, so
            // match by name regardless of kind.
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !VCS_DIRS.contains(&name))
        })
        .build()
}

/// Report the ignore sources configured for the filesystem walk and the
/// repository `.gitignore` files it can consult.
pub fn ignore_sources(root: &Path) -> Result<IgnoreSources> {
    let mut files = parent_gitignore_files(root);

    let walker = filesystem_walker(root);

    for entry in walker {
        let entry = entry?;
        if entry.file_name() == ".gitignore" {
            files.push(display_relative(root, entry.path()));
        }
    }
    files.sort();
    files.dedup();

    Ok(IgnoreSources {
        gitignore: IgnoreSource {
            enabled: true,
            files: files.clone(),
        },
        git_exclude: IgnoreSource {
            enabled: false,
            files: Vec::new(),
        },
        git_global: IgnoreSource {
            enabled: false,
            files: Vec::new(),
        },
    })
}

fn parent_gitignore_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in root.ancestors() {
        let candidate = dir.join(".gitignore");
        if candidate.is_file() {
            files.push(display_relative(root, &candidate));
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    files
}

fn display_relative(root: &Path, path: &Path) -> String {
    if let Ok(relative) = path.strip_prefix(root) {
        return relative.to_string_lossy().replace('\\', "/");
    }

    let root_parts: Vec<_> = root.components().collect();
    let path_parts: Vec<_> = path.components().collect();
    let shared = root_parts
        .iter()
        .zip(&path_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in shared..root_parts.len() {
        relative.push("..");
    }
    for part in &path_parts[shared..] {
        relative.push(part.as_os_str());
    }
    relative.to_string_lossy().replace('\\', "/")
}

/// `lstat` the entry at `root/relative` without following symlinks.
fn abs_lstat(root: &Path, relative: &str) -> Option<std::fs::Metadata> {
    root.join(relative).symlink_metadata().ok()
}

/// Whether an entry is excluded by the `ignore` globs. Files match the path
/// as-is. A directory is also excluded when a glob matches its path with a
/// trailing slash — so `examples/**` (which matches `examples/`, not `examples`)
/// drops the `examples` directory node, while `docs/*.md` leaves `docs` intact.
fn is_ignored(ignore_set: &Option<GlobSet>, relative: &str, kind: NodeKind) -> bool {
    let Some(set) = ignore_set else {
        return false;
    };
    set.is_match(relative) || (kind == NodeKind::Dir && set.is_match(format!("{relative}/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_git(path: &Path) {
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

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
        // `target/**` drops both the build file and the `target` directory node
        // (it matches `target/`), leaving only the kept file.
        assert_eq!(paths, vec!["keep.md"]);
    }

    #[test]
    fn yields_directory_nodes() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("guides")).unwrap();
        fs::write(dir.path().join("guides/intro.md"), "i").unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let guides = files.iter().find(|f| f.path == "guides").unwrap();
        assert_eq!(guides.kind, NodeKind::Dir);
        assert!(guides.bytes.is_none(), "directories carry no content");

        let intro = files.iter().find(|f| f.path == "guides/intro.md").unwrap();
        assert_eq!(intro.kind, NodeKind::File);
    }

    #[test]
    fn includes_empty_directories() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        assert!(
            files
                .iter()
                .any(|f| f.path == "empty" && f.kind == NodeKind::Dir)
        );
    }

    #[test]
    fn root_is_not_a_node() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        assert!(
            !files.iter().any(|f| f.path.is_empty()),
            "the graph root must not appear as a node"
        );
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

    #[test]
    fn repository_gitignore_above_root_prunes_the_walk() {
        let outer = TempDir::new().unwrap();
        init_git(outer.path());
        fs::write(outer.path().join(".gitignore"), "/project/ignored.md\n").unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("ignored.md"), "x").unwrap();
        fs::write(root.join("keep.md"), "k").unwrap();

        let files = walk(&root, &[]).unwrap();
        assert!(
            files.iter().any(|f| f.path == "keep.md"),
            "the unmatched file must remain in the graph"
        );
        assert!(
            !files.iter().any(|f| f.path == "ignored.md"),
            "a repository-root anchored rule must apply below the graph root"
        );
    }

    #[test]
    fn per_clone_git_exclude_does_not_prune_the_walk() {
        let repo = TempDir::new().unwrap();
        init_git(repo.path());
        fs::write(repo.path().join(".git/info/exclude"), "local.md\n").unwrap();
        fs::write(repo.path().join("local.md"), "local").unwrap();

        let files = walk(repo.path(), &[]).unwrap();
        assert!(
            files.iter().any(|file| file.path == "local.md"),
            "machine-local excludes must not change the graph"
        );
    }

    #[test]
    fn ignore_report_names_repository_files_and_disabled_sources() {
        let outer = TempDir::new().unwrap();
        init_git(outer.path());
        fs::write(outer.path().join(".gitignore"), "outside.md\n").unwrap();
        let root = outer.path().join("project");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        fs::write(root.join("docs/.gitignore"), "draft.md\n").unwrap();

        let report = ignore_sources(&root).unwrap();
        assert_eq!(
            report.gitignore.files,
            vec!["../.gitignore", ".gitignore", "docs/.gitignore"]
        );
        assert!(report.gitignore.enabled);
        assert!(!report.git_exclude.enabled);
        assert!(!report.git_global.enabled);
    }

    #[test]
    fn dot_dirs_are_walked_but_vcs_dirs_are_pruned() {
        let dir = TempDir::new().unwrap();
        // A real version-control store: pruned, contents and all.
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git").join("HEAD"), "ref: x").unwrap();
        // An ordinary dot-directory: part of the graph.
        fs::create_dir(dir.path().join(".github")).unwrap();
        fs::write(dir.path().join(".github").join("ci.yml"), "y").unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

        assert!(
            !paths.iter().any(|p| p.starts_with(".git/") || *p == ".git"),
            "VCS metadata must be pruned, got: {paths:?}"
        );
        assert!(
            paths.contains(&".github") && paths.contains(&".github/ci.yml"),
            "ordinary dot-directories must be walked, got: {paths:?}"
        );
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

    #[cfg(unix)]
    #[test]
    fn symlink_to_directory_is_typed_symlink() {
        // Indirection wins over target kind: a symlink pointing at a directory is
        // a `Symlink`, not a `Dir`. The link's target resolves through the edge
        // the builder emits, not the node's type.
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let alias = files.iter().find(|f| f.path == "alias").unwrap();
        assert_eq!(alias.kind, NodeKind::Symlink);
        let real = files.iter().find(|f| f.path == "real").unwrap();
        assert_eq!(real.kind, NodeKind::Dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_to_file_carries_no_bytes() {
        // A symlink is pure indirection: its node is never hashed. Content lives
        // at the real path; the builder's edge carries the relationship.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("real.md"), "content").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.md"), dir.path().join("alias.md"))
            .unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let alias = files.iter().find(|f| f.path == "alias.md").unwrap();
        assert_eq!(alias.kind, NodeKind::Symlink);
        assert!(alias.bytes.is_none(), "symlink node must carry no bytes");
        let real = files.iter().find(|f| f.path == "real.md").unwrap();
        assert_eq!(real.bytes.as_deref(), Some(&b"content"[..]));
    }

    #[cfg(unix)]
    #[test]
    fn does_not_descend_into_symlinked_directory() {
        // A symlinked directory is a leaf node, not a second copy of the subtree.
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("real")).unwrap();
        fs::write(dir.path().join("real/child.md"), "c").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();

        let files = walk(dir.path(), &[]).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"alias"), "the symlink itself is a node");
        assert!(
            paths.contains(&"real/child.md"),
            "real content appears once"
        );
        assert!(
            !paths.contains(&"alias/child.md"),
            "must not re-walk the target subtree through the symlink, got: {paths:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn does_not_leak_content_through_escaping_directory_symlink() {
        // A symlink to a directory outside the root must not pull outside files
        // into the graph as readable nodes.
        let outer = TempDir::new().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::create_dir(outer.path().join("secrets")).unwrap();
        fs::write(outer.path().join("secrets/passwd.md"), "TOP SECRET").unwrap();
        std::os::unix::fs::symlink(outer.path().join("secrets"), root.join("alias")).unwrap();

        let files = walk(&root, &[]).unwrap();
        assert!(
            !files.iter().any(|f| f.path.contains("passwd")),
            "outside content must not be walked through a symlink"
        );
    }
}
