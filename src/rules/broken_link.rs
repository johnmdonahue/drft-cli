use crate::analyses::Analysis;
use crate::analyses::edge_classification::{EdgeClassification, EdgeStatus};
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct BrokenLinkRule;

impl Rule for BrokenLinkRule {
    fn name(&self) -> &str {
        "broken-link"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = EdgeClassification.run(graph, root);

        result
            .edges
            .iter()
            .filter_map(|e| match &e.status {
                EdgeStatus::Broken => Some(Diagnostic {
                    rule: "broken-link".into(),
                    message: "file not found".into(),
                    source: Some(e.source.clone()),
                    target: Some(e.target.clone()),
                    fix: Some(format!(
                        "{} does not exist \u{2014} either create it or update the link in {}",
                        e.target, e.source
                    )),
                    ..Default::default()
                }),
                EdgeStatus::Excluded => Some(Diagnostic {
                    rule: "broken-link".into(),
                    message: "file excluded by ignore pattern".into(),
                    source: Some(e.source.clone()),
                    target: Some(e.target.clone()),
                    fix: Some(format!(
                        "{} exists but is excluded by an ignore pattern \u{2014} either remove the link from {} or update the ignore config",
                        e.target, e.source
                    )),
                    ..Default::default()
                }),
                _ => None,
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
    fn detects_broken_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = BrokenLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "broken-link");
        assert_eq!(diagnostics[0].source.as_deref(), Some("index.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("gone.md"));
    }

    #[test]
    fn no_diagnostic_for_valid_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();
        fs::write(dir.path().join("setup.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = BrokenLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }
}
