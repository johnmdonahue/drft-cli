//! The `fs` source: a `.gitignore`-aware filesystem walk that yields one
//! [`SourceFile`] per file, symlink, and directory under the graph root.

use anyhow::{Context, Result, bail};
use globset::GlobSet;
use ignore::{Walk, WalkBuilder};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
    pub dot_ignore: IgnoreSource,
    pub git_exclude: IgnoreSource,
    pub git_global: IgnoreSource,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct IgnoreSource {
    pub enabled: bool,
    pub files: Vec<String>,
}

/// Resolved ignore policy shared by traversal and `config --show-ignores`.
/// Git supplies the effective machine-local paths; the `ignore` crate supplies
/// matching and source precedence.
struct IgnorePolicy {
    repository_root: Option<PathBuf>,
    git_exclude: Option<PathBuf>,
    git_global: Option<PathBuf>,
}

/// Walk the tree under `root`, honoring Git's ignore sources and the configured
/// `ignore` globs, yielding one [`SourceFile`] per file, symlink, and directory.
/// Paths are relative to `root`, sorted.
///
/// Hidden entries are *not* skipped: a dot-directory like `.github/` is part of
/// the graph. The lone exception is VCS metadata ([`VCS_DIRS`]), pruned from
/// traversal — `.git/` would otherwise flood the graph with internal state.
///
/// Repository `.gitignore` rules from the graph root through the repository
/// root prune the walk. Nested `.gitignore` files also apply to their subtrees.
/// In Git repositories, the effective `.git/info/exclude` and
/// `core.excludesFile` sources apply with Git's precedence.
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

    let policy = resolve_ignore_policy(root)?;
    let walker = filesystem_walker(root, &policy);

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

fn filesystem_walker(root: &Path, policy: &IgnorePolicy) -> Walk {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false)
        .ignore(false)
        // The effective Git paths are added explicitly below. `ignore` 0.4's
        // built-in global resolver does not read repository-local Git config.
        .git_global(false)
        .git_exclude(false);

    if let Some(repository_root) = &policy.repository_root {
        builder.current_dir(repository_root);
    }
    // Explicit ignore files have the lowest precedence and later files win.
    // Git's order is repository `.gitignore`, info/exclude, global excludes,
    // so add the global file first and info/exclude second.
    if let Some(path) = policy.git_global.as_ref().filter(|path| path.is_file()) {
        let _ = builder.add_ignore(path);
    }
    if let Some(path) = policy.git_exclude.as_ref().filter(|path| path.is_file()) {
        let _ = builder.add_ignore(path);
    }

    builder
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
    let policy = resolve_ignore_policy(root)?;
    let mut files = Vec::new();

    if let Some(repository_root) = &policy.repository_root {
        files = parent_gitignore_files(root, repository_root);
        for entry in filesystem_walker(root, &policy) {
            let entry = entry?;
            if entry.file_type().is_some_and(|kind| kind.is_dir()) {
                let candidate = entry.path().join(".gitignore");
                if candidate.is_file() {
                    files.push(display_relative(root, &candidate));
                }
            }
        }
    }
    files.sort();
    files.dedup();

    Ok(IgnoreSources {
        gitignore: IgnoreSource {
            enabled: policy.repository_root.is_some(),
            files: files.clone(),
        },
        dot_ignore: IgnoreSource {
            enabled: false,
            files: Vec::new(),
        },
        git_exclude: IgnoreSource {
            enabled: policy.git_exclude.is_some(),
            files: Vec::new(),
        },
        git_global: IgnoreSource {
            enabled: policy.git_global.is_some(),
            files: Vec::new(),
        },
    })
}

fn resolve_ignore_policy(root: &Path) -> Result<IgnorePolicy> {
    let repository_root = repository_root(root);
    let Some(git_root) = repository_root
        .as_ref()
        .filter(|candidate| candidate.join(".git").exists())
    else {
        return Ok(IgnorePolicy {
            repository_root,
            git_exclude: None,
            git_global: None,
        });
    };

    let git_exclude = git_path(git_root, &["rev-parse", "--git-path", "info/exclude"])?;
    let configured_global = git_optional_path(
        git_root,
        &["config", "-z", "--path", "--get", "core.excludesFile"],
    )?;
    let git_global = configured_global
        .or_else(ignore::gitignore::gitconfig_excludes_path)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                git_root.join(path)
            }
        });

    Ok(IgnorePolicy {
        repository_root,
        git_exclude: Some(git_exclude),
        git_global,
    })
}

fn git_path(root: &Path, args: &[&str]) -> Result<PathBuf> {
    let output = run_git(root, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed for {}: {}",
            args.join(" "),
            root.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    output_path(root, &output.stdout, b'\n')
}

fn git_optional_path(root: &Path, args: &[&str]) -> Result<Option<PathBuf>> {
    let output = run_git(root, args)?;
    if output.status.success() {
        return output_path(root, &output.stdout, b'\0').map(Some);
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!(
        "git {} failed for {}: {}",
        args.join(" "),
        root.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

fn run_git(root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git for {}", root.display()))
}

fn output_path(root: &Path, bytes: &[u8], terminator: u8) -> Result<PathBuf> {
    let bytes = bytes.strip_suffix(&[terminator]).unwrap_or(bytes);
    let bytes = if terminator == b'\n' {
        bytes.strip_suffix(b"\r").unwrap_or(bytes)
    } else {
        bytes
    };
    let text = std::str::from_utf8(bytes).context("git returned a non-UTF-8 path")?;
    if text.is_empty() {
        bail!("git returned an empty path for {}", root.display());
    }
    let path = PathBuf::from(text);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn repository_root(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .find(|dir| dir.join(".git").exists() || dir.join(".jj").exists())
        .map(Path::to_path_buf)
}

fn parent_gitignore_files(root: &Path, repository_root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for dir in root.ancestors() {
        let candidate = dir.join(".gitignore");
        if candidate.is_file() {
            files.push(display_relative(root, &candidate));
        }
        if dir == repository_root {
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

    fn assert_git_ignores(root: &Path, path: &str) {
        let status = Command::new("git")
            .args(["check-ignore", "-q", path])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "Git must ignore the fixture {path}");
    }

    fn git_ignores(root: &Path, path: &str) -> bool {
        Command::new("git")
            .args(["check-ignore", "-q", path])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
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
        init_git(dir.path());
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
    fn git_info_exclude_matches_git_discovery() {
        let repo = TempDir::new().unwrap();
        init_git(repo.path());
        fs::write(repo.path().join(".git/info/exclude"), "local.md\n").unwrap();
        fs::write(repo.path().join("local.md"), "local").unwrap();
        fs::write(repo.path().join("keep.md"), "keep").unwrap();

        assert_git_ignores(repo.path(), "local.md");

        let files = walk(repo.path(), &[]).unwrap();
        assert!(
            !files.iter().any(|file| file.path == "local.md"),
            "drft must omit the path Git excludes"
        );
        assert!(files.iter().any(|file| file.path == "keep.md"));
    }

    #[test]
    fn git_info_exclude_matches_git_with_a_separate_git_dir() {
        let fixture = TempDir::new().unwrap();
        let repo = fixture.path().join("repo");
        let git_dir = fixture.path().join("metadata");
        fs::create_dir(&repo).unwrap();
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .arg("--separate-git-dir")
            .arg(&git_dir)
            .arg(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(git_dir.join("info/exclude"), "local.md\n").unwrap();
        fs::write(repo.join("local.md"), "local").unwrap();
        fs::write(repo.join("keep.md"), "keep").unwrap();

        assert_git_ignores(&repo, "local.md");

        let files = walk(&repo, &[]).unwrap();
        assert!(!files.iter().any(|file| file.path == "local.md"));
        assert!(files.iter().any(|file| file.path == "keep.md"));
    }

    #[test]
    fn effective_core_excludes_file_matches_git_discovery() {
        let fixture = TempDir::new().unwrap();
        let repo = fixture.path().join("repo");
        fs::create_dir(&repo).unwrap();
        init_git(&repo);
        let excludes = fixture.path().join("effective-ignore");
        fs::write(&excludes, "machine-only.md\n").unwrap();
        let status = Command::new("git")
            .args(["config", "--local", "core.excludesFile"])
            .arg(&excludes)
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(repo.join("machine-only.md"), "machine").unwrap();
        fs::write(repo.join("keep.md"), "keep").unwrap();

        assert_git_ignores(&repo, "machine-only.md");

        let files = walk(&repo, &[]).unwrap();
        assert!(!files.iter().any(|file| file.path == "machine-only.md"));
        assert!(files.iter().any(|file| file.path == "keep.md"));
    }

    #[test]
    fn included_core_excludes_file_matches_git_discovery() {
        let fixture = TempDir::new().unwrap();
        let repo = fixture.path().join("repo");
        fs::create_dir(&repo).unwrap();
        init_git(&repo);
        let excludes = fixture.path().join("included-ignore");
        fs::write(&excludes, "included-only.md\n").unwrap();
        let included_config = fixture.path().join("included-config");
        fs::write(
            &included_config,
            format!("[core]\nexcludesFile = {}\n", excludes.display()),
        )
        .unwrap();
        let status = Command::new("git")
            .args(["config", "--local", "include.path"])
            .arg(&included_config)
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(repo.join("included-only.md"), "included").unwrap();
        fs::write(repo.join("keep.md"), "keep").unwrap();

        assert_git_ignores(&repo, "included-only.md");

        let files = walk(&repo, &[]).unwrap();
        assert!(!files.iter().any(|file| file.path == "included-only.md"));
        assert!(files.iter().any(|file| file.path == "keep.md"));
    }

    #[test]
    fn git_ignore_source_precedence_matches_git() {
        let fixture = TempDir::new().unwrap();
        let repo = fixture.path().join("repo");
        fs::create_dir(&repo).unwrap();
        init_git(&repo);

        let paths = [
            "global-ignore-info-negate.md",
            "info-ignore-repository-negate.md",
            "global-negate-info-ignore.md",
            "info-negate-repository-ignore.md",
        ];
        for path in paths {
            fs::write(repo.join(path), path).unwrap();
        }

        let excludes = fixture.path().join("effective-ignore");
        fs::write(
            &excludes,
            "global-ignore-info-negate.md\n!global-negate-info-ignore.md\n",
        )
        .unwrap();
        let status = Command::new("git")
            .args(["config", "--local", "core.excludesFile"])
            .arg(&excludes)
            .current_dir(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(
            repo.join(".git/info/exclude"),
            "!global-ignore-info-negate.md\ninfo-ignore-repository-negate.md\nglobal-negate-info-ignore.md\n!info-negate-repository-ignore.md\n",
        )
        .unwrap();
        fs::write(
            repo.join(".gitignore"),
            "!info-ignore-repository-negate.md\ninfo-negate-repository-ignore.md\n",
        )
        .unwrap();

        let files = walk(&repo, &[]).unwrap();
        for path in paths {
            let drft_ignores = !files.iter().any(|file| file.path == path);
            assert_eq!(
                drft_ignores,
                git_ignores(&repo, path),
                "drft and Git disagree for {path}"
            );
        }
    }

    #[test]
    fn dot_ignore_above_repository_does_not_prune_the_walk() {
        let outer = TempDir::new().unwrap();
        fs::write(outer.path().join(".ignore"), "hidden.md\n").unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir(&repo).unwrap();
        init_git(&repo);
        let root = repo.join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("hidden.md"), "visible").unwrap();

        let files = walk(&root, &[]).unwrap();
        assert!(
            files.iter().any(|file| file.path == "hidden.md"),
            "a .ignore file outside the repository must not change the graph"
        );
    }

    #[test]
    fn ignore_report_names_repository_files_and_enabled_git_sources() {
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
        assert!(!report.dot_ignore.enabled);
        assert!(report.git_exclude.enabled);
        assert!(report.git_global.enabled);
    }

    #[test]
    fn ignore_report_names_a_consulted_gitignore_hidden_as_a_file() {
        let repo = TempDir::new().unwrap();
        init_git(repo.path());
        fs::write(repo.path().join(".gitignore"), "/docs/.gitignore\n").unwrap();
        fs::create_dir(repo.path().join("docs")).unwrap();
        fs::write(repo.path().join("docs/.gitignore"), "secret.md\n").unwrap();
        fs::write(repo.path().join("docs/secret.md"), "secret").unwrap();

        let files = walk(repo.path(), &[]).unwrap();
        assert!(!files.iter().any(|file| file.path == "docs/secret.md"));
        let report = ignore_sources(repo.path()).unwrap();
        assert_eq!(
            report.gitignore.files,
            vec![".gitignore", "docs/.gitignore"]
        );
    }

    #[test]
    fn ignore_report_disables_gitignore_outside_a_repository() {
        let outer = TempDir::new().unwrap();
        fs::write(outer.path().join(".gitignore"), "parent-hidden.md\n").unwrap();
        let root = outer.path().join("project");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/.gitignore"), "nested-hidden.md\n").unwrap();
        fs::write(root.join("parent-hidden.md"), "visible").unwrap();
        fs::write(root.join("docs/nested-hidden.md"), "visible").unwrap();

        let files = walk(&root, &[]).unwrap();
        assert!(files.iter().any(|file| file.path == "parent-hidden.md"));
        assert!(
            files
                .iter()
                .any(|file| file.path == "docs/nested-hidden.md")
        );
        let report = ignore_sources(&root).unwrap();
        assert!(!report.gitignore.enabled);
        assert!(report.gitignore.files.is_empty());
    }

    #[test]
    fn dot_dirs_are_walked_but_vcs_dirs_are_pruned() {
        let dir = TempDir::new().unwrap();
        // A real version-control store: pruned, contents and all.
        init_git(dir.path());
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
