use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct FragilityRule;

impl Rule for FragilityRule {
    fn name(&self) -> &str {
        "fragility"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.bridges;
        let mut diagnostics = Vec::new();

        for vertex in &result.cut_vertices {
            diagnostics.push(Diagnostic {
                rule: "fragility".into(),
                message: "cut vertex".into(),
                node: Some(vertex.clone()),
                fix: Some(format!(
                    "{vertex} is a single point of failure \u{2014} removing it disconnects the graph. Consider adding alternative paths."
                )),
                ..Default::default()
            });
        }

        for bridge in &result.bridges {
            diagnostics.push(Diagnostic {
                rule: "fragility".into(),
                message: "bridge edge".into(),
                source: Some(bridge.source.clone()),
                target: Some(bridge.target.clone()),
                fix: Some(format!(
                    "{} \u{2194} {} is the only connection between two parts of the graph \u{2014} consider adding alternative paths",
                    bridge.source, bridge.target
                )),
                ..Default::default()
            });
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_enriched, make_node};
    use crate::rules::RuleContext;

    #[test]
    fn no_fragility_in_cycle() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = FragilityRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_cut_vertex_and_bridge() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = FragilityRule.evaluate(&ctx);
        let cut_vertices: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message == "cut vertex")
            .collect();
        let bridges: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message == "bridge edge")
            .collect();
        assert_eq!(cut_vertices.len(), 1);
        assert_eq!(bridges.len(), 2);
    }
}
