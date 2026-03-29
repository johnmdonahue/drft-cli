use crate::analyses::Analysis;
use crate::analyses::edge_classification::{EdgeClassification, EdgeStatus};
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct DirectoryLinkRule;

impl Rule for DirectoryLinkRule {
    fn name(&self) -> &str {
        "directory-link"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = EdgeClassification.run(graph, root);

        result
            .edges
            .iter()
            .filter_map(|e| match &e.status {
                EdgeStatus::DirectoryTarget => Some(Diagnostic {
                    rule: "directory-link".into(),
                    message: "links to directory, not file".into(),
                    source: Some(e.source.clone()),
                    target: Some(e.target.clone()),
                    fix: Some(format!(
                        "{}/ is a directory \u{2014} link to the specific file (e.g., {}/README.md)",
                        e.target.trim_end_matches('/'),
                        e.target.trim_end_matches('/')
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
    fn detects_directory_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();
        let guides = dir.path().join("guides");
        fs::create_dir(&guides).unwrap();
        fs::write(guides.join("README.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "guides".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = DirectoryLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "directory-link");
        assert_eq!(diagnostics[0].target.as_deref(), Some("guides"));
    }

    #[test]
    fn no_diagnostic_for_file_link() {
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

        let rule = DirectoryLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }
}
