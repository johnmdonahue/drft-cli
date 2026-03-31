use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct FragmentationRule;

impl Rule for FragmentationRule {
    fn name(&self) -> &str {
        "fragmentation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.connected_components;

        if result.component_count <= 1 {
            return Vec::new();
        }

        result
            .components
            .iter()
            .skip(1)
            .map(|c| {
                let members = c.members.join(", ");
                Diagnostic {
                    rule: "fragmentation".into(),
                    message: format!("disconnected component ({} nodes)", c.members.len()),
                    node: c.members.first().cloned(),
                    fix: Some(format!(
                        "these nodes are disconnected from the main graph: {members} \u{2014} add links to connect them"
                    )),
                    ..Default::default()
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
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::rules::RuleContext;

    fn make_enriched(graph: Graph) -> EnrichedGraph {
        crate::analyses::enrich_graph(graph, std::path::Path::new("."), &Config::defaults(), None)
    }

    #[test]
    fn no_diagnostic_when_connected() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = FragmentationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_fragmentation() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = FragmentationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "fragmentation");
        assert!(diagnostics[0].message.contains("disconnected component"));
    }

    #[test]
    fn no_diagnostic_for_empty_graph() {
        let graph = Graph::new();
        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = FragmentationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
