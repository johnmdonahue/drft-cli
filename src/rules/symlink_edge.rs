use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct SymlinkEdgeRule;

impl Rule for SymlinkEdgeRule {
    fn name(&self) -> &str {
        "symlink-edge"
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

                // Check if target path is a symlink on the filesystem
                let target_path = root.join(&edge.target);
                if target_path.is_symlink() {
                    let resolved = std::fs::read_link(&target_path)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "unknown".to_string());
                    Some(Diagnostic {
                        rule: "symlink-edge".into(),
                        message: format!("target is a symlink to {resolved}"),
                        source: Some(edge.source.clone()),
                        target: Some(edge.target.clone()),
                        fix: Some(format!(
                            "{} is a symlink to {resolved} \u{2014} consider linking to the actual file directly in {}",
                            edge.target, edge.source
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
    use std::os::unix::fs::symlink;
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
    fn detects_symlink_target() {
        let dir = TempDir::new().unwrap();
        let shared = dir.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::write(shared.join("setup.md"), "# Setup").unwrap();
        symlink(shared.join("setup.md"), dir.path().join("setup.md")).unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
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
        let diagnostics = SymlinkEdgeRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "symlink-edge");
        assert!(diagnostics[0].message.contains("symlink"));
    }

    #[test]
    fn no_diagnostic_for_regular_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
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
        let diagnostics = SymlinkEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
