use crate::diagnostic::Diagnostic;
use crate::graph::{Graph, NodeType};
use crate::rules::Rule;
use std::path::Path;

pub struct OrphanRule;

impl Rule for OrphanRule {
    fn name(&self) -> &str {
        "orphan"
    }

    fn evaluate(&self, graph: &Graph, _root: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for (node_id, node) in &graph.nodes {
            // Skip synthetic nodes (frontier, virtual, external)
            if matches!(
                node.node_type,
                NodeType::Frontier | NodeType::Virtual | NodeType::External
            ) {
                continue;
            }

            // A node with no inbound edges is an orphan
            let has_inbound = graph
                .reverse
                .get(node_id.as_str())
                .is_some_and(|edges| !edges.is_empty());

            if !has_inbound {
                diagnostics.push(Diagnostic {
                    rule: "orphan".into(),
                    message: "no inbound links".into(),
                    node: Some(node_id.clone()),
                    fix: Some(format!(
                        "{node_id} has no inbound links — either link to it from another file or remove it"
                    )),
                    ..Default::default()
                });
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::Document,
            hash: None,
        }
    }

    #[test]
    fn detects_orphan() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("orphan.md"));
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = OrphanRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        assert!(orphan_nodes.contains(&"orphan.md"));
        // index.md is also an orphan (nothing links to it), which is expected
        assert!(orphan_nodes.contains(&"index.md"));
    }

    #[test]
    fn linked_file_is_not_orphan() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("setup.md"));
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = OrphanRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        // setup.md has an inbound link — not an orphan
        assert!(!orphan_nodes.contains(&"setup.md"));
        // index.md has no inbound links — is an orphan
        assert!(orphan_nodes.contains(&"index.md"));
    }
}
