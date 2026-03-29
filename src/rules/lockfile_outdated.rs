use std::path::Path;

use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::lockfile::{Lockfile, read_lockfile};
use crate::rules::Rule;

pub struct LockfileOutdatedRule;

impl Rule for LockfileOutdatedRule {
    fn name(&self) -> &str {
        "lockfile-outdated"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        // Skip if no lockfile exists — nothing to compare against
        let existing = match read_lockfile(root) {
            Ok(Some(lf)) => lf,
            _ => return vec![],
        };

        // Build what the lockfile would be from the current graph.
        // Preserve the existing manifest (same logic as run_lock without flags).
        let manifest = existing.manifest.clone();
        let current = Lockfile::from_graph(graph, manifest);

        // Compare serialized TOML — byte-for-byte like lock --check
        let existing_toml = match existing.to_toml() {
            Ok(t) => t,
            Err(_) => return vec![],
        };
        let current_toml = match current.to_toml() {
            Ok(t) => t,
            Err(_) => return vec![],
        };

        if existing_toml == current_toml {
            return vec![];
        }

        vec![Diagnostic {
            rule: "lockfile-outdated".into(),
            message: "lockfile does not match current graph".into(),
            fix: Some(
                "the dependency graph has changed since the last lock — run drft lock to update"
                    .into(),
            ),
            ..Default::default()
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::build_graph;
    use crate::lockfile::write_lockfile;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn clean_when_lockfile_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let config = crate::config::Config::defaults();
        let graph = build_graph(dir.path(), &config).unwrap();
        let lockfile = Lockfile::from_graph(&graph, None);
        write_lockfile(dir.path(), &lockfile).unwrap();

        let rule = LockfileOutdatedRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_new_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();

        let config = crate::config::Config::defaults();
        let graph = build_graph(dir.path(), &config).unwrap();
        let lockfile = Lockfile::from_graph(&graph, None);
        write_lockfile(dir.path(), &lockfile).unwrap();

        // Add a new file
        fs::write(dir.path().join("new.md"), "# New").unwrap();

        let graph2 = build_graph(dir.path(), &config).unwrap();
        let rule = LockfileOutdatedRule;
        let diagnostics = rule.evaluate(&graph2, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "lockfile-outdated");
    }

    #[test]
    fn detects_changed_links() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let config = crate::config::Config::defaults();
        let graph = build_graph(dir.path(), &config).unwrap();
        let lockfile = Lockfile::from_graph(&graph, None);
        write_lockfile(dir.path(), &lockfile).unwrap();

        // Edit to add a link
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();

        let graph2 = build_graph(dir.path(), &config).unwrap();
        let rule = LockfileOutdatedRule;
        let diagnostics = rule.evaluate(&graph2, dir.path());
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn skips_when_no_lockfile() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "# Hello").unwrap();

        let config = crate::config::Config::defaults();
        let graph = build_graph(dir.path(), &config).unwrap();

        let rule = LockfileOutdatedRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }
}
