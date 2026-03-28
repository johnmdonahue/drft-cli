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
        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            // External URLs are not checked for existence
            if edge.target.starts_with("http://") || edge.target.starts_with("https://") {
                continue;
            }

            // Skip edges to known nodes (they exist and are in the graph)
            if graph.nodes.contains_key(&edge.target) {
                continue;
            }

            let target_path = root.join(&edge.target);
            if target_path.exists() {
                // File exists on disk but is not in the graph — excluded by ignore pattern
                diagnostics.push(Diagnostic {
                    rule: "broken-link".into(),
                    message: "file excluded by ignore pattern".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} exists but is excluded by an ignore pattern — either remove the link from {} or update the ignore config",
                        edge.target, edge.source
                    )),
                    ..Default::default()
                });
            } else {
                diagnostics.push(Diagnostic {
                    rule: "broken-link".into(),
                    message: "file not found".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} does not exist — either create it or update the link in {}",
                        edge.target, edge.source
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
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_broken_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();
        // gone.md does not exist

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
