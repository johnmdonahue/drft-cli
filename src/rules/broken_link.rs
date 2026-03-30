use crate::diagnostic::Diagnostic;
use crate::graph::NodeType;
use crate::rules::{Rule, RuleContext};

pub struct BrokenLinkRule;

impl Rule for BrokenLinkRule {
    fn name(&self) -> &str {
        "broken-link"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = ctx.graph;
        let root = ctx.root;

        graph
            .edges
            .iter()
            .filter_map(|edge| {
                // Skip external URLs
                if edge.target.starts_with("http://") || edge.target.starts_with("https://") {
                    return None;
                }

                // If target exists in graph, it's valid
                if let Some(node) = graph.nodes.get(&edge.target) {
                    if node.node_type == NodeType::Graph {
                        return None; // Frontier nodes are valid
                    }
                    // Check if target is a symlink (not broken)
                    let target_path = root.join(&edge.target);
                    if target_path.is_symlink() {
                        return None; // Handled by indirect-link rule
                    }
                    return None; // Valid
                }

                // Target not in graph — filesystem checks
                let target_path = root.join(&edge.target);

                if target_path.is_dir() {
                    return None; // Handled by directory-link rule
                }

                if target_path.is_symlink() {
                    return None; // Handled by indirect-link rule
                }

                if target_path.exists() {
                    // File exists but was excluded by ignore pattern
                    return Some(Diagnostic {
                        rule: "broken-link".into(),
                        message: "file excluded by ignore pattern".into(),
                        source: Some(edge.source.clone()),
                        target: Some(edge.target.clone()),
                        fix: Some(format!(
                            "{} exists but is excluded by an ignore pattern \u{2014} either remove the link from {} or update the ignore config",
                            edge.target, edge.source
                        )),
                        ..Default::default()
                    });
                }

                // Truly broken
                Some(Diagnostic {
                    rule: "broken-link".into(),
                    message: "file not found".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} does not exist \u{2014} either create it or update the link in {}",
                        edge.target, edge.source
                    )),
                    ..Default::default()
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_ctx<'a>(graph: &'a Graph, root: &'a Path, config: &'a Config) -> RuleContext<'a> {
        RuleContext {
            graph,
            root,
            config,
            lockfile: None,
        }
    }

    #[test]
    fn detects_broken_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = BrokenLinkRule.evaluate(&ctx);
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
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = BrokenLinkRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
