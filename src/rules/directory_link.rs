use crate::diagnostic::Diagnostic;
use crate::graph::{Graph, NodeType};
use crate::rules::Rule;
use std::path::Path;

pub struct DirectoryLinkRule;

impl Rule for DirectoryLinkRule {
    fn name(&self) -> &str {
        "directory-link"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            // Skip edges to frontier nodes (implicit virtual→frontier)
            if let Some(target_node) = graph.nodes.get(&edge.target)
                && target_node.node_type == NodeType::Frontier
            {
                continue;
            }
            let target_path = root.join(&edge.target);
            if target_path.is_dir() {
                diagnostics.push(Diagnostic {
                    rule: "directory-link".into(),
                    message: "links to directory, not file".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{}/ is a directory — link to the specific file (e.g., {}/README.md)",
                        edge.target.trim_end_matches('/'),
                        edge.target.trim_end_matches('/')
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
