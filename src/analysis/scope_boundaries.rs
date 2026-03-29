use super::{Analysis, Metric, MetricKind};
use crate::graph::{Graph, NodeType};
use crate::lockfile::read_lockfile;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeEscape {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EncapsulationViolation {
    pub source: String,
    pub target: String,
    pub scope: String,
    pub manifest_file: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeBoundariesResult {
    pub sealed: bool,
    pub escapes: Vec<ScopeEscape>,
    pub encapsulation_violations: Vec<EncapsulationViolation>,
}

pub struct ScopeBoundaries;

impl Analysis for ScopeBoundaries {
    type Output = ScopeBoundariesResult;

    fn name(&self) -> &str {
        "scope-boundaries"
    }

    fn run(&self, graph: &Graph, root: &Path) -> ScopeBoundariesResult {
        let sealed = root.join("drft.lock").exists();

        // Find scope escapes (edges with ../ targets)
        let escapes = if sealed {
            graph
                .edges
                .iter()
                .filter(|edge| {
                    !edge.target.starts_with("http://")
                        && !edge.target.starts_with("https://")
                        && (edge.target.starts_with("../") || edge.target == "..")
                })
                .map(|edge| ScopeEscape {
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // Find encapsulation violations
        let mut encapsulation_violations = Vec::new();

        for (path, node) in &graph.nodes {
            if node.node_type != NodeType::Frontier {
                continue;
            }

            let child_dir = root.join(path.trim_end_matches('/'));
            let child_lockfile = match read_lockfile(&child_dir) {
                Ok(Some(lf)) => lf,
                _ => continue,
            };
            let manifest = match &child_lockfile.manifest {
                Some(m) => m,
                None => continue,
            };

            let scope_prefix = path.as_str();

            for edge in &graph.edges {
                // Skip virtual sources (implicit virtual→frontier edges)
                if let Some(source_node) = graph.nodes.get(&edge.source)
                    && source_node.node_type == NodeType::Virtual
                {
                    continue;
                }

                if !edge.target.starts_with(scope_prefix) {
                    continue;
                }

                let relative_target = &edge.target[scope_prefix.len()..];
                if !manifest.nodes.iter().any(|n| n == relative_target) {
                    encapsulation_violations.push(EncapsulationViolation {
                        source: edge.source.clone(),
                        target: edge.target.clone(),
                        scope: scope_prefix.to_string(),
                        manifest_file: manifest.file.clone(),
                    });
                }
            }
        }

        ScopeBoundariesResult {
            sealed,
            escapes,
            encapsulation_violations,
        }
    }

    fn metrics(&self, output: &ScopeBoundariesResult, _graph: &Graph) -> Vec<Metric> {
        vec![
            Metric {
                name: "escape_count".into(),
                value: output.escapes.len() as f64,
                kind: MetricKind::Count,
                dimension: "consistency".into(),
            },
            Metric {
                name: "encapsulation_violation_count".into(),
                value: output.encapsulation_violations.len() as f64,
                kind: MetricKind::Count,
                dimension: "consistency".into(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Graph, Node, NodeType};
    use crate::lockfile::{Lockfile, LockfileNode, Manifest, write_lockfile};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_scope_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_edge(make_edge("index.md", "../README.md"));

        let result = ScopeBoundaries.run(&graph, dir.path());
        assert!(result.sealed);
        assert_eq!(result.escapes.len(), 1);
        assert_eq!(result.escapes[0].target, "../README.md");
    }

    #[test]
    fn no_escape_without_lockfile() {
        let dir = TempDir::new().unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_edge(make_edge("index.md", "../README.md"));

        let result = ScopeBoundaries.run(&graph, dir.path());
        assert!(!result.sealed);
        assert!(result.escapes.is_empty());
    }

    #[test]
    fn detects_encapsulation_violation() {
        let dir = TempDir::new().unwrap();
        let research = dir.path().join("research");
        fs::create_dir_all(&research).unwrap();
        fs::write(research.join("overview.md"), "# Overview").unwrap();
        fs::write(research.join("internal.md"), "# Internal").unwrap();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "overview.md".into(),
            LockfileNode {
                node_type: NodeType::Document,
                hash: Some("b3:aaa".into()),
            },
        );
        let lockfile = Lockfile {
            lockfile_version: 1,
            manifest: Some(Manifest {
                file: "overview.md".into(),
                nodes: vec!["overview.md".into()],
            }),
            nodes,
            edges: vec![],
        };
        write_lockfile(&research, &lockfile).unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Frontier,
            hash: None,
        });
        graph.add_edge(make_edge("index.md", "research/internal.md"));

        let result = ScopeBoundaries.run(&graph, dir.path());
        assert_eq!(result.encapsulation_violations.len(), 1);
        assert_eq!(
            result.encapsulation_violations[0].target,
            "research/internal.md"
        );
        assert_eq!(result.encapsulation_violations[0].scope, "research/");
    }

    #[test]
    fn manifest_file_is_not_violation() {
        let dir = TempDir::new().unwrap();
        let research = dir.path().join("research");
        fs::create_dir_all(&research).unwrap();
        fs::write(research.join("overview.md"), "# Overview").unwrap();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "overview.md".into(),
            LockfileNode {
                node_type: NodeType::Document,
                hash: Some("b3:aaa".into()),
            },
        );
        let lockfile = Lockfile {
            lockfile_version: 1,
            manifest: Some(Manifest {
                file: "overview.md".into(),
                nodes: vec!["overview.md".into()],
            }),
            nodes,
            edges: vec![],
        };
        write_lockfile(&research, &lockfile).unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Frontier,
            hash: None,
        });
        graph.add_edge(make_edge("index.md", "research/overview.md"));

        let result = ScopeBoundaries.run(&graph, dir.path());
        assert!(result.encapsulation_violations.is_empty());
    }
}
