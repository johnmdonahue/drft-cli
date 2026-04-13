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

                // Check if the target node is a symlink
                let target_node = graph.nodes.get(&edge.target)?;
                if target_node.node_type != Some(crate::graph::NodeType::Symlink) {
                    return None;
                }

                // Find the filesystem edge from this node to get the symlink target
                let symlink_target = graph.forward.get(&edge.target)
                    .and_then(|indices| indices.iter()
                        .find_map(|&idx| {
                            let fs_edge = &graph.edges[idx];
                            if fs_edge.parser == "filesystem" { Some(fs_edge.target.clone()) } else { None }
                        }));

                let resolved = symlink_target.as_deref().unwrap_or("unknown");
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
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::{make_enriched, make_node};
    use crate::graph::{Edge, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    #[test]
    fn detects_symlink_target() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: Some(NodeType::Symlink),
            included: true,
            hash: None,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "/shared/setup.md".into(),
            node_type: Some(NodeType::File),
            included: false,
            hash: None,
            metadata: HashMap::new(),
        });
        // Filesystem edge from symlink to its target
        graph.add_edge(Edge {
            source: "setup.md".into(),
            target: "/shared/setup.md".into(),
            link: None,
            parser: "filesystem".into(),
        });
        // Content edge from index.md to the symlink
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
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: Some(NodeType::File),
            included: false,
            hash: None,
            metadata: HashMap::new(),
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
