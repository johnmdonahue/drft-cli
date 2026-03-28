use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct IndirectLinkRule;

impl Rule for IndirectLinkRule {
    fn name(&self) -> &str {
        "indirect-link"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            if edge.target.starts_with("http://") || edge.target.starts_with("https://") {
                continue;
            }

            let target_path = root.join(&edge.target);
            if target_path.is_symlink() {
                let resolved = match std::fs::read_link(&target_path) {
                    Ok(p) => p.to_string_lossy().to_string(),
                    Err(_) => "unknown".to_string(),
                };
                diagnostics.push(Diagnostic {
                    rule: "indirect-link".into(),
                    message: format!("target is a symlink to {resolved}"),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} is a symlink to {resolved} — consider linking to the actual file directly in {}",
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
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn detects_symlink_target() {
        let dir = TempDir::new().unwrap();
        let shared = dir.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::write(shared.join("setup.md"), "# Setup").unwrap();
        symlink(shared.join("setup.md"), dir.path().join("setup.md")).unwrap();

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

        let rule = IndirectLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "indirect-link");
        assert!(diagnostics[0].message.contains("symlink"));
    }

    #[test]
    fn no_diagnostic_for_regular_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

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

        let rule = IndirectLinkRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }
}
