use crate::analysis::Analysis;
use crate::analysis::scope_boundaries::ScopeBoundaries;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct EncapsulationRule;

impl Rule for EncapsulationRule {
    fn name(&self) -> &str {
        "encapsulation"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = ScopeBoundaries.run(graph, root);

        result
            .encapsulation_violations
            .iter()
            .map(|v| Diagnostic {
                rule: "encapsulation".into(),
                message: format!("not in {}manifest", v.scope),
                source: Some(v.source.clone()),
                target: Some(v.target.clone()),
                fix: Some(format!(
                    "{} is not exposed by the {}manifest \u{2014} either add it to the manifest or remove the link from {}",
                    v.target, v.scope, v.source
                )),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
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
