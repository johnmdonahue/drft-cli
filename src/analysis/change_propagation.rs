use super::{Analysis, Metric, MetricKind};
use crate::discovery::find_child_scopes;
use crate::graph::{Graph, NodeType, hash_bytes};
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
pub struct BoundaryChange {
    pub node: String,
    pub reason: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangePropagationResult {
    pub has_lockfile: bool,
    pub directly_changed: Vec<DirectChange>,
    pub transitively_stale: Vec<TransitiveStale>,
    pub boundary_changes: Vec<BoundaryChange>,
}

pub struct ChangePropagation;

impl Analysis for ChangePropagation {
    type Output = ChangePropagationResult;

    fn name(&self) -> &str {
        "change-propagation"
    }

    fn run(&self, _graph: &Graph, root: &Path) -> ChangePropagationResult {
        let lockfile = match read_lockfile(root) {
            Ok(Some(lf)) => lf,
            _ => {
                return ChangePropagationResult {
                    has_lockfile: false,
                    directly_changed: Vec::new(),
                    transitively_stale: Vec::new(),
                    boundary_changes: Vec::new(),
                };
            }
        };

        // Direct changes: hash comparison
        let mut directly_stale: HashSet<String> = HashSet::new();
        let mut directly_changed = Vec::new();

        for (path, locked_node) in &lockfile.nodes {
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

        // Boundary changes
        let mut boundary_changes = Vec::new();
        let current_scopes: HashSet<String> = find_child_scopes(root)
            .unwrap_or_default()
            .into_iter()
            .collect();

        for (path, node) in &lockfile.nodes {
            if node.node_type == NodeType::Frontier && !current_scopes.contains(path.as_str()) {
                boundary_changes.push(BoundaryChange {
                    node: path.clone(),
                    reason: "scope removed".into(),
                });
            }
        }

        let lockfile_frontiers: HashSet<&str> = lockfile
            .nodes
            .iter()
            .filter(|(_, n)| n.node_type == NodeType::Frontier)
            .map(|(p, _)| p.as_str())
            .collect();
        for scope in &current_scopes {
            if !lockfile_frontiers.contains(scope.as_str()) {
                boundary_changes.push(BoundaryChange {
                    node: scope.clone(),
                    reason: "new scope".into(),
                });
            }
        }

        // Transitive staleness: BFS over reverse dependency edges from lockfile
        let mut transitively_stale = Vec::new();

        if !directly_stale.is_empty() {
            let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
            for edge in &lockfile.edges {
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
        boundary_changes.sort_by(|a, b| a.node.cmp(&b.node));

        ChangePropagationResult {
            has_lockfile: true,
            directly_changed,
            transitively_stale,
            boundary_changes,
        }
    }

    fn metrics(&self, output: &ChangePropagationResult, _graph: &Graph) -> Vec<Metric> {
        if !output.has_lockfile {
            return vec![];
        }

        vec![
            Metric {
                name: "directly_changed_count".into(),
                value: output.directly_changed.len() as f64,
                kind: MetricKind::Count,
                dimension: "timeliness".into(),
            },
            Metric {
                name: "transitively_stale_count".into(),
                value: output.transitively_stale.len() as f64,
                kind: MetricKind::Count,
                dimension: "timeliness".into(),
            },
            Metric {
                name: "boundary_change_count".into(),
                value: output.boundary_changes.len() as f64,
                kind: MetricKind::Count,
                dimension: "timeliness".into(),
            },
        ]
    }
}

fn compute_current_hash(root: &Path, relative_path: &str) -> Option<String> {
    if relative_path.ends_with('/') {
        let child_dir = root.join(relative_path.trim_end_matches('/'));
        let lockfile_path = child_dir.join("drft.lock");
        let content = std::fs::read(&lockfile_path).ok()?;
        Some(hash_bytes(&content))
    } else {
        let full_path = root.join(relative_path);
        let content = std::fs::read(&full_path).ok()?;
        Some(hash_bytes(&content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::lockfile::{Lockfile, write_lockfile};
    use std::fs;
    use tempfile::TempDir;

    fn setup_locked_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let mut graph = Graph::new();
        let index_hash = hash_bytes(b"[setup](setup.md)");
        let setup_hash = hash_bytes(b"# Setup");

        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: Some(index_hash),
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Document,
            hash: Some(setup_hash),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let lockfile = Lockfile::from_graph(&graph, None);
        write_lockfile(dir.path(), &lockfile).unwrap();
        dir
    }

    #[test]
    fn no_changes_when_unchanged() {
        let dir = setup_locked_dir();
        let graph = Graph::new();
        let result = ChangePropagation.run(&graph, dir.path());
        assert!(result.has_lockfile);
        assert!(result.directly_changed.is_empty());
        assert!(result.transitively_stale.is_empty());
    }

    #[test]
    fn detects_direct_and_transitive() {
        let dir = setup_locked_dir();
        fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

        let graph = Graph::new();
        let result = ChangePropagation.run(&graph, dir.path());
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
        let result = ChangePropagation.run(&graph, dir.path());
        assert!(!result.has_lockfile);
        assert!(result.directly_changed.is_empty());
    }

    #[test]
    fn detects_deleted_file() {
        let dir = setup_locked_dir();
        fs::remove_file(dir.path().join("setup.md")).unwrap();

        let graph = Graph::new();
        let result = ChangePropagation.run(&graph, dir.path());
        assert_eq!(result.directly_changed.len(), 1);
        assert_eq!(result.directly_changed[0].reason, "file deleted");
    }
}
