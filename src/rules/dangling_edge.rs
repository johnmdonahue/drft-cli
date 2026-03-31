use crate::diagnostic::Diagnostic;
use crate::graph::NodeType;
use crate::rules::{Rule, RuleContext};

pub struct DanglingEdgeRule;

impl Rule for DanglingEdgeRule {
    fn name(&self) -> &str {
        "dangling-edge"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;

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
                    if edge.target_is_symlink {
                        return None; // Handled by symlink-edge rule
                    }
                    return None; // Valid
                }

                // Target not in graph — check edge properties
                if edge.target_is_symlink {
                    return None; // Handled by symlink-edge rule
                }

                if edge.target_is_directory {
                    return None; // Handled by directory-edge rule
                }

                // Truly broken — file not found
                // (If the file existed on disk, build_graph would have created an External node)
                Some(Diagnostic {
                    rule: "dangling-edge".into(),
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
    use crate::analyses::EnrichedGraph;
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::rules::RuleContext;

    fn make_enriched(graph: Graph) -> EnrichedGraph {
        crate::analyses::enrich_graph(graph, std::path::Path::new("."), &Config::defaults(), None)
    }

    #[test]
    fn detects_dangling_edge() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
            target_is_symlink: false,
            target_is_directory: false,
            symlink_target: None,
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DanglingEdgeRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "dangling-edge");
        assert_eq!(diagnostics[0].source.as_deref(), Some("index.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("gone.md"));
    }

    #[test]
    fn no_diagnostic_for_valid_link() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
            target_is_symlink: false,
            target_is_directory: false,
            symlink_target: None,
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DanglingEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn skips_symlink_targets() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "linked.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
            target_is_symlink: true,
            target_is_directory: false,
            symlink_target: Some("real.md".into()),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DanglingEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn skips_directory_targets() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "guides".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
            target_is_symlink: false,
            target_is_directory: true,
            symlink_target: None,
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DanglingEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
