use super::{Analysis, AnalysisContext};
use crate::graph::hash_bytes;
use crate::lockfile::read_lockfile;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DirectChange {
    pub node: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TransitiveStale {
    pub node: String,
    pub via: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangePropagationResult {
    pub has_lockfile: bool,
    pub directly_changed: Vec<DirectChange>,
    pub transitively_stale: Vec<TransitiveStale>,
}

pub struct ChangePropagation;

impl Analysis for ChangePropagation {
    type Output = ChangePropagationResult;

    fn name(&self) -> &str {
        "change-propagation"
    }

    fn run(&self, ctx: &AnalysisContext) -> ChangePropagationResult {
        let graph = ctx.graph;
        let root = ctx.root;
        let lockfile = match read_lockfile(root) {
            Ok(Some(lf)) => lf,
            _ => {
                return ChangePropagationResult {
                    has_lockfile: false,
                    directly_changed: Vec::new(),
                    transitively_stale: Vec::new(),
                };
            }
        };

        // Direct changes: hash comparison
        let mut directly_stale: HashSet<String> = HashSet::new();
        let mut directly_changed = Vec::new();

        for (path, locked_node) in &lockfile.nodes {
            // Directory nodes are existence-only leaves with no hash.
            if locked_node.hash.is_none() {
                continue;
            }
            let current_hash = compute_current_hash(root, path);
            match (&locked_node.hash, &current_hash) {
                (Some(locked), Some(current)) if locked != current => {
                    directly_stale.insert(path.clone());
                    directly_changed.push(DirectChange {
                        node: path.clone(),
                        reason: "content changed".into(),
                    });
                }
                (Some(_), None) => {
                    directly_stale.insert(path.clone());
                    directly_changed.push(DirectChange {
                        node: path.clone(),
                        reason: "file deleted".into(),
                    });
                }
                _ => {}
            }
        }

        // Transitive staleness: BFS over reverse dependency edges from current graph
        let mut transitively_stale = Vec::new();

        if !directly_stale.is_empty() {
            let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
            for edge in &graph.edges {
                dependents
                    .entry(edge.target.as_str())
                    .or_default()
                    .push(edge.source.as_str());
            }

            let mut stale_via: HashMap<String, String> = HashMap::new();
            let mut queue: VecDeque<String> = directly_stale.iter().cloned().collect();

            while let Some(stale_node) = queue.pop_front() {
                if let Some(deps) = dependents.get(stale_node.as_str()) {
                    for &dependent in deps {
                        if !stale_via.contains_key(dependent) && !directly_stale.contains(dependent)
                        {
                            stale_via.insert(dependent.to_string(), stale_node.clone());
                            queue.push_back(dependent.to_string());
                        }
                    }
                }
            }

            let mut stale_pairs: Vec<_> = stale_via.into_iter().collect();
            stale_pairs.sort_by(|a, b| a.0.cmp(&b.0));

            transitively_stale = stale_pairs
                .into_iter()
                .map(|(node, via)| TransitiveStale { node, via })
                .collect();
        }

        directly_changed.sort_by(|a, b| a.node.cmp(&b.node));

        ChangePropagationResult {
            has_lockfile: true,
            directly_changed,
            transitively_stale,
        }
    }
}

fn compute_current_hash(root: &Path, relative_path: &str) -> Option<String> {
    let full_path = root.join(relative_path);
    let content = std::fs::read(&full_path).ok()?;
    Some(hash_bytes(&content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::test_helpers::make_edge;
    use crate::graph::{Graph, Node};
    use crate::lockfile::{Lockfile, write_lockfile};
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx<'a>(graph: &'a Graph, root: &'a Path, config: &'a Config) -> AnalysisContext<'a> {
        AnalysisContext {
            graph,
            root,
            config,
            lockfile: None,
        }
    }

    fn setup_locked_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let mut graph = Graph::new();
        let index_hash = hash_bytes(b"[setup](setup.md)");
        let setup_hash = hash_bytes(b"# Setup");

        graph.add_node(Node {
            path: "index.md".into(),
            hash: Some(index_hash),
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            hash: Some(setup_hash),
            metadata: HashMap::new(),
        });
        graph.add_edge(make_edge("index.md", "setup.md"));

        let lockfile = Lockfile::from_graph(&graph);
        write_lockfile(dir.path(), &lockfile).unwrap();
        dir
    }

    #[test]
    fn no_changes_when_unchanged() {
        let dir = setup_locked_dir();
        let graph = Graph::new();
        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = ChangePropagation.run(&ctx);
        assert!(result.has_lockfile);
        assert!(result.directly_changed.is_empty());
        assert!(result.transitively_stale.is_empty());
    }

    #[test]
    fn detects_direct_and_transitive() {
        let dir = setup_locked_dir();
        fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

        let config = Config::defaults();
        let graph = crate::graph::build_graph(dir.path(), &config).unwrap();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = ChangePropagation.run(&ctx);
        assert_eq!(result.directly_changed.len(), 1);
        assert_eq!(result.directly_changed[0].node, "setup.md");
        assert_eq!(result.transitively_stale.len(), 1);
        assert_eq!(result.transitively_stale[0].node, "index.md");
        assert_eq!(result.transitively_stale[0].via, "setup.md");
    }

    #[test]
    fn no_lockfile_returns_empty() {
        let dir = TempDir::new().unwrap();
        let graph = Graph::new();
        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = ChangePropagation.run(&ctx);
        assert!(!result.has_lockfile);
        assert!(result.directly_changed.is_empty());
    }

    #[test]
    fn detects_deleted_file() {
        let dir = setup_locked_dir();
        fs::remove_file(dir.path().join("setup.md")).unwrap();

        let graph = Graph::new();
        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = ChangePropagation.run(&ctx);
        assert_eq!(result.directly_changed.len(), 1);
        assert_eq!(result.directly_changed[0].reason, "file deleted");
    }
}
