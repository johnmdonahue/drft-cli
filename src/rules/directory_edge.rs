use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct DirectoryEdgeRule;

impl Rule for DirectoryEdgeRule {
    fn name(&self) -> &str {
        "directory-edge"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;

        graph
            .edges
            .iter()
            .filter_map(|edge| {
                // Skip URIs
                if crate::graph::is_uri(&edge.target) {
                    return None;
                }

                // If target is in the graph as a known node, skip
                if graph.nodes.contains_key(&edge.target) {
                    return None;
                }

                // Check target properties set during graph building
                if graph.target_properties.get(&edge.target).is_some_and(|p| p.is_directory) {
                    Some(Diagnostic {
                        rule: "directory-edge".into(),
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
    use crate::analyses::EnrichedGraph;
    use crate::config::Config;
    use crate::graph::{Edge, Graph, Node, NodeType, TargetProperties};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    fn make_enriched(graph: Graph) -> EnrichedGraph {
        crate::analyses::enrich_graph(graph, std::path::Path::new("."), &Config::defaults(), None)
    }

    #[test]
    fn detects_directory_link() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.target_properties.insert(
            "guides".into(),
            TargetProperties {
                is_symlink: false,
                is_directory: true,
                symlink_target: None,
            },
        );
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "guides".into(),
            link: None, parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DirectoryEdgeRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "directory-edge");
        assert_eq!(diagnostics[0].target.as_deref(), Some("guides"));
    }

    #[test]
    fn no_diagnostic_for_file_link() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            link: None, parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = DirectoryEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
