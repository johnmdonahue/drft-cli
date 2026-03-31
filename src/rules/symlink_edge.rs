use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct SymlinkEdgeRule;

impl Rule for SymlinkEdgeRule {
    fn name(&self) -> &str {
        "symlink-edge"
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

                // Check edge property set during graph building
                if edge.target_is_symlink {
                    let resolved = edge
                        .symlink_target
                        .as_deref()
                        .unwrap_or("unknown");
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
    use crate::analyses::EnrichedGraph;
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    fn make_enriched(graph: Graph) -> EnrichedGraph {
        crate::analyses::enrich_graph(graph, std::path::Path::new("."), &Config::defaults(), None)
    }

    #[test]
    fn detects_symlink_target() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
            target_is_symlink: true,
            target_is_directory: false,
            symlink_target: Some("/shared/setup.md".into()),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = SymlinkEdgeRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "symlink-edge");
        assert!(diagnostics[0].message.contains("symlink"));
    }

    #[test]
    fn no_diagnostic_for_regular_file() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
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
        let diagnostics = SymlinkEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
