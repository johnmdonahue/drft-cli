use crate::diagnostic::Diagnostic;
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
                // Skip URIs
                if crate::graph::is_uri(&edge.target) {
                    return None;
                }

                let props = graph.target_properties.get(&edge.target);

                // If target exists in graph, it's valid
                if graph.nodes.contains_key(&edge.target) {
                    return None;
                }

                // Target not in graph — check target properties
                if props.is_some_and(|p| p.is_symlink) {
                    return None; // Handled by symlink-edge rule
                }

                // Truly broken — file not found
                // (If the target existed on disk, build_graph would have created a node)
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
    use crate::graph::test_helpers::make_enriched;
    use crate::graph::{Edge, Graph, Node, NodeType, TargetProperties};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    #[test]
    fn detects_dangling_edge() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            link: None,
            parser: "markdown".into(),
            
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
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
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            link: None,
            parser: "markdown".into(),
            
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
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
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        });
        graph.target_properties.insert(
            "linked.md".into(),
            TargetProperties {
                is_symlink: true,
                is_directory: false,
                symlink_target: Some("real.md".into()),
            },
        );
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "linked.md".into(),
            link: None,
            parser: "markdown".into(),
            
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
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
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        });
        // Directories now get proper Directory nodes in the graph
        graph.add_node(Node {
            path: "guides".into(),
            node_type: NodeType::Directory,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "guides".into(),
            link: None,
            parser: "markdown".into(),
            
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = DanglingEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
