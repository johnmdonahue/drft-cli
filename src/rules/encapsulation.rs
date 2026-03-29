use crate::diagnostic::Diagnostic;
use crate::graph::{Graph, NodeType};
use crate::lockfile::read_lockfile;
use crate::rules::Rule;
use std::path::Path;

pub struct EncapsulationRule;

impl Rule for EncapsulationRule {
    fn name(&self) -> &str {
        "encapsulation"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // For each frontier node, check if the child scope is sealed
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
                None => continue, // unsealed — no encapsulation to enforce
            };

            let scope_prefix = path.as_str(); // e.g., "research/"

            for edge in &graph.edges {
                // Only check edges from non-virtual sources (virtual→frontier is implicit)
                if let Some(source_node) = graph.nodes.get(&edge.source)
                    && source_node.node_type == NodeType::Virtual
                {
                    continue;
                }

                if !edge.target.starts_with(scope_prefix) {
                    continue;
                }

                // Target relative to child scope (e.g., "internal.md")
                let relative_target = &edge.target[scope_prefix.len()..];
                if !manifest.nodes.iter().any(|n| n == relative_target) {
                    diagnostics.push(Diagnostic {
                        rule: "encapsulation".into(),
                        message: format!("not in {scope_prefix}manifest"),
                        source: Some(edge.source.clone()),
                        target: Some(edge.target.clone()),
                        fix: Some(format!(
                            "{} is not exposed by the {scope_prefix}manifest — either add it to the manifest or remove the link from {}",
                            edge.target, edge.source
                        )),
                        ..Default::default()
                    });
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node};
    use crate::lockfile::{Lockfile, LockfileNode, Manifest, write_lockfile};
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    fn setup_sealed_child(dir: &Path) {
        let research = dir.join("research");
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
        nodes.insert(
            "internal.md".into(),
            LockfileNode {
                node_type: NodeType::Document,
                hash: Some("b3:bbb".into()),
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
    }

    #[test]
    fn no_violation_for_manifest_file() {
        let dir = TempDir::new().unwrap();
        setup_sealed_child(dir.path());

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Frontier,
            hash: None,
        });
        graph.add_node(Node {
            path: "research/overview.md".into(),
            node_type: NodeType::Virtual,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/overview.md".into(),
            edge_type: EdgeType::Inline,
        });
        // Implicit virtual → frontier
        graph.add_edge(Edge {
            source: "research/overview.md".into(),
            target: "research/".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = EncapsulationRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn violation_for_non_manifest_file() {
        let dir = TempDir::new().unwrap();
        setup_sealed_child(dir.path());

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Frontier,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/internal.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = EncapsulationRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "encapsulation");
        assert_eq!(
            diagnostics[0].target.as_deref(),
            Some("research/internal.md")
        );
    }
}
