use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::diagnostic::Diagnostic;
use crate::discovery::find_child_scopes;
use crate::graph::{hash_bytes, Graph, NodeType};
use crate::lockfile::read_lockfile;
use crate::rules::Rule;

pub struct StaleRule;

impl Rule for StaleRule {
    fn name(&self) -> &str {
        "stale"
    }

    fn evaluate(&self, _graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        // Load the stored lockfile — skip silently if none exists
        let lockfile = match read_lockfile(root) {
            Ok(Some(lf)) => lf,
            _ => return vec![],
        };

        // Find directly stale nodes: current hash differs from lockfile hash
        let mut directly_stale: HashSet<String> = HashSet::new();
        for (path, locked_node) in &lockfile.nodes {
            let current_hash = compute_current_hash(root, path);
            match (&locked_node.hash, &current_hash) {
                (Some(locked), Some(current)) if locked != current => {
                    directly_stale.insert(path.clone());
                }
                (Some(_), None) => {
                    // File was deleted — counts as changed
                    directly_stale.insert(path.clone());
                }
                _ => {}
            }
        }

        // Detect scope boundary changes
        let mut boundary_diagnostics = Vec::new();
        let current_scopes: HashSet<String> = find_child_scopes(root)
            .unwrap_or_default()
            .into_iter()
            .collect();

        // Frontier nodes in lockfile whose child scope no longer exists
        for (path, node) in &lockfile.nodes {
            if node.node_type == NodeType::Frontier && !current_scopes.contains(path.as_str()) {
                boundary_diagnostics.push(Diagnostic {
                        rule: "stale".into(),
                        message: "scope boundary changed".into(),
                        node: Some(path.clone()),
                        fix: Some(format!(
                            "{path} no longer has a drft.lock — run drft lock to update the parent lockfile"
                        )),
                        ..Default::default()
                    });
            }
        }

        // New child scopes not in the lockfile
        let lockfile_frontiers: HashSet<&str> = lockfile
            .nodes
            .iter()
            .filter(|(_, n)| n.node_type == NodeType::Frontier)
            .map(|(p, _)| p.as_str())
            .collect();
        for scope in &current_scopes {
            if !lockfile_frontiers.contains(scope.as_str()) {
                boundary_diagnostics.push(Diagnostic {
                    rule: "stale".into(),
                    message: "scope boundary changed".into(),
                    node: Some(scope.clone()),
                    fix: Some(format!(
                        "{scope} is a new child scope — run drft lock to update the parent lockfile"
                    )),
                    ..Default::default()
                });
            }
        }

        if directly_stale.is_empty() && boundary_diagnostics.is_empty() {
            return vec![];
        }

        if directly_stale.is_empty() {
            boundary_diagnostics.sort_by(|a, b| a.node.cmp(&b.node));
            return boundary_diagnostics;
        }

        // Build reverse dependency map from lockfile edges:
        // If A links to B (A depends on B), then when B changes, A is stale.
        // So: stale_node (B) -> dependents (A) who need to be flagged.
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &lockfile.edges {
            dependents
                .entry(edge.target.as_str())
                .or_default()
                .push(edge.source.as_str());
        }

        // BFS: propagate staleness from directly stale nodes to their dependents
        let mut stale_via: HashMap<String, String> = HashMap::new();
        let mut queue: VecDeque<String> = VecDeque::new();

        for node in &directly_stale {
            queue.push_back(node.clone());
        }

        while let Some(stale_node) = queue.pop_front() {
            if let Some(deps) = dependents.get(stale_node.as_str()) {
                for &dependent in deps {
                    if !stale_via.contains_key(dependent) && !directly_stale.contains(dependent) {
                        stale_via.insert(dependent.to_string(), stale_node.clone());
                        queue.push_back(dependent.to_string());
                    }
                }
            }
        }

        // Emit diagnostics for directly stale nodes
        let mut diagnostics: Vec<Diagnostic> = directly_stale
            .iter()
            .map(|node| {
                Diagnostic {
                    rule: "stale".into(),
                    message: "content changed".into(),
                    node: Some(node.clone()),
                    fix: Some(format!(
                        "{node} has been modified since the last lock — review its dependents, then run drft lock"
                    )),
                    ..Default::default()
                }
            })
            .collect();

        // Emit diagnostics for transitively stale nodes
        diagnostics.extend(stale_via.into_iter().map(|(node, via)| {
            let fix = format!(
                "{via} has changed — review {node} to ensure it still accurately reflects {via}, then run drft lock"
            );
            Diagnostic {
                rule: "stale".into(),
                message: "stale via".into(),
                node: Some(node),
                via: Some(via),
                fix: Some(fix),
                ..Default::default()
            }
        }));

        diagnostics.extend(boundary_diagnostics);
        diagnostics.sort_by(|a, b| a.node.cmp(&b.node));
        diagnostics
    }
}

fn compute_current_hash(root: &Path, relative_path: &str) -> Option<String> {
    if relative_path.ends_with('/') {
        // Frontier node: hash the child scope's lockfile
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
    use crate::lockfile::{write_lockfile, Lockfile};
    use std::fs;
    use tempfile::TempDir;

    fn setup_locked_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        // Build graph and lockfile
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
    fn no_staleness_when_unchanged() {
        let dir = setup_locked_dir();
        let graph = Graph::new(); // stale rule reads lockfile directly
        let rule = StaleRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_direct_and_transitive_staleness() {
        let dir = setup_locked_dir();
        // Edit setup.md
        fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

        let graph = Graph::new();
        let rule = StaleRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 2);

        // Direct: setup.md changed
        let direct = diagnostics
            .iter()
            .find(|d| d.message == "content changed")
            .unwrap();
        assert_eq!(direct.node.as_deref(), Some("setup.md"));
        assert!(direct.via.is_none());

        // Transitive: index.md depends on setup.md
        let transitive = diagnostics
            .iter()
            .find(|d| d.message == "stale via")
            .unwrap();
        assert_eq!(transitive.node.as_deref(), Some("index.md"));
        assert_eq!(transitive.via.as_deref(), Some("setup.md"));
    }

    #[test]
    fn skips_when_no_lockfile() {
        let dir = TempDir::new().unwrap();
        let graph = Graph::new();
        let rule = StaleRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn deleted_file_causes_staleness() {
        let dir = setup_locked_dir();
        fs::remove_file(dir.path().join("setup.md")).unwrap();

        let graph = Graph::new();
        let rule = StaleRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 2);

        // Direct: setup.md deleted
        let direct = diagnostics
            .iter()
            .find(|d| d.message == "content changed")
            .unwrap();
        assert_eq!(direct.node.as_deref(), Some("setup.md"));

        // Transitive: index.md depends on setup.md
        let transitive = diagnostics
            .iter()
            .find(|d| d.message == "stale via")
            .unwrap();
        assert_eq!(transitive.node.as_deref(), Some("index.md"));
        assert_eq!(transitive.via.as_deref(), Some("setup.md"));
    }
}
