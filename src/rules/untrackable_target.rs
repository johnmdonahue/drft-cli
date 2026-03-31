use crate::diagnostic::Diagnostic;
use crate::graph::NodeType;
use crate::rules::{Rule, RuleContext};

pub struct UntrackableTargetRule;

impl Rule for UntrackableTargetRule {
    fn name(&self) -> &str {
        "untrackable-target"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;

        graph
            .edges
            .iter()
            .filter_map(|edge| {
                let node = graph.nodes.get(&edge.target)?;
                if node.node_type != NodeType::Directory || node.hash.is_some() {
                    return None;
                }

                Some(Diagnostic {
                    rule: "untrackable-target".into(),
                    message: "directory has no lockfile — cannot track for staleness".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "lock it (drft init -C {t} && drft lock -C {t}) or link to a specific file",
                        t = edge.target
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
    use crate::graph::{Edge, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    #[test]
    fn detects_untrackable_directory() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "guides".into(),
            node_type: NodeType::Directory,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
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
        let diagnostics = UntrackableTargetRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "untrackable-target");
        assert_eq!(diagnostics[0].target.as_deref(), Some("guides"));
    }

    #[test]
    fn no_diagnostic_for_directory_with_lockfile() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Directory,
            hash: Some("b3:abc".into()),
            graph: None,
            is_graph: true,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/".into(),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = UntrackableTargetRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn no_diagnostic_for_file_link() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
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
        let diagnostics = UntrackableTargetRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
