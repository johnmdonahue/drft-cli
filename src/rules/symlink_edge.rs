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
                // Skip URIs
                if crate::graph::is_uri(&edge.target) {
                    return None;
                }

                // Check target properties set during graph building
                let props = graph.target_properties.get(&edge.target);
                if props.is_some_and(|p| p.is_symlink) {
                    let resolved = props
                        .and_then(|p| p.symlink_target.as_deref())
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
    use crate::graph::test_helpers::make_enriched;
    use crate::graph::{Edge, Graph, Node, NodeType, TargetProperties};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    #[test]
    fn detects_symlink_target() {
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
            "setup.md".into(),
            TargetProperties {
                is_symlink: true,
                is_directory: false,
                symlink_target: Some("/shared/setup.md".into()),
            },
        );
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
        let diagnostics = SymlinkEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
