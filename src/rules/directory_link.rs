use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

/// See `docs/rules/directory-link.md` for details.
pub struct DirectoryLinkRule;

impl Rule for DirectoryLinkRule {
    fn name(&self) -> &str {
        "directory-link"
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

                // If target is in the graph as a known node, skip
                if graph.nodes.contains_key(&edge.target) {
                    return None;
                }

                // Check if target is a directory on the filesystem
                let target_path = root.join(&edge.target);
                if target_path.is_dir() {
                    Some(Diagnostic {
                        rule: "directory-link".into(),
                        message: "links to directory, not file".into(),
                        source: Some(edge.source.clone()),
                        target: Some(edge.target.clone()),
                        fix: Some(format!(
                            "{}/ is a directory \u{2014} link to the specific file (e.g., {}/README.md)",
                            edge.target.trim_end_matches('/'),
                            edge.target.trim_end_matches('/')
                        )),
                        ..Default::default()
                    })
                } else {
                    None
                }
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
    fn detects_directory_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();
        let guides = dir.path().join("guides");
        fs::create_dir(&guides).unwrap();
        fs::write(guides.join("README.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "guides".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = DirectoryLinkRule.evaluate(&ctx);
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
        let diagnostics = DirectoryLinkRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
