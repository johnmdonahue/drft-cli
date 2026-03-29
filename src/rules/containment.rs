use crate::analyses::Analysis;
use crate::analyses::scope_boundaries::ScopeBoundaries;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct ContainmentRule;

impl Rule for ContainmentRule {
    fn name(&self) -> &str {
        "containment"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = ScopeBoundaries.run(graph, root);

        if !result.sealed {
            return vec![];
        }

        result
            .escapes
            .iter()
            .map(|e| Diagnostic {
                rule: "containment".into(),
                message: "links outside scope boundary".into(),
                source: Some(e.source.clone()),
                target: Some(e.target.clone()),
                fix: Some(format!(
                    "link reaches outside the scope \u{2014} move {} into the scope or remove the link from {}",
                    e.target, e.source
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
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../README.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "containment");
        assert_eq!(diagnostics[0].target.as_deref(), Some("../README.md"));
    }

    #[test]
    fn detects_deep_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../../other.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn no_violation_for_internal_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn vacuous_without_lockfile() {
        let dir = TempDir::new().unwrap();

        let mut graph = Graph::new();
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../escape.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(
            diagnostics.is_empty(),
            "no lockfile means no boundary to enforce"
        );
    }
}
