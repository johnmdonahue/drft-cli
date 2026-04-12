use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct OrphanNodeRule;

impl Rule for OrphanNodeRule {
    fn name(&self) -> &str {
        "orphan-node"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.degree;

        result
            .nodes
            .iter()
            .filter(|nd| nd.in_degree == 0 && nd.out_degree == 0)
            .map(|nd| Diagnostic {
                rule: "orphan-node".into(),
                message: "no connections".into(),
                node: Some(nd.node.clone()),
                fix: Some(format!(
                    "{} has no inbound or outbound links — either link to it from another file or remove it",
                    nd.node
                )),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_enriched, make_node};
    use crate::graph::{Edge, Resolution, TargetKind};
    use crate::rules::RuleContext;

    #[test]
    fn detects_isolated_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("orphan.md"));
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            target_kind: TargetKind::Internal(Resolution::Missing),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = OrphanNodeRule.evaluate(&ctx);

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        assert!(orphan_nodes.contains(&"orphan.md"));
    }

    #[test]
    fn root_node_is_not_orphan() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("setup.md"));
        graph.add_edge(make_edge("index.md", "setup.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = OrphanNodeRule.evaluate(&ctx);

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        assert!(!orphan_nodes.contains(&"setup.md"));
        assert!(!orphan_nodes.contains(&"index.md"));
    }
}
