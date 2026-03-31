use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct RedundantEdgeRule;

impl Rule for RedundantEdgeRule {
    fn name(&self) -> &str {
        "redundant-edge"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.transitive_reduction;

        result
            .redundant_edges
            .iter()
            .map(|re| Diagnostic {
                rule: "redundant-edge".into(),
                message: "transitively redundant".into(),
                source: Some(re.source.clone()),
                target: Some(re.target.clone()),
                via: Some(re.via.clone()),
                fix: Some(format!(
                    "{} links directly to {}, but already reaches it via {} \u{2014} remove the direct link",
                    re.source, re.target, re.via
                )),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::EnrichedGraph;
    use crate::config::Config;
    use crate::graph::{Edge, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            link: None,
            parser: "markdown".into(),
        }
    }

    fn make_enriched(graph: Graph) -> EnrichedGraph {
        crate::analyses::enrich_graph(graph, std::path::Path::new("."), &Config::defaults(), None)
    }

    #[test]
    fn produces_diagnostics_for_redundant_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = RedundantEdgeRule.evaluate(&ctx);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "redundant-edge");
        assert_eq!(diagnostics[0].source.as_deref(), Some("a.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("c.md"));
        assert_eq!(diagnostics[0].via.as_deref(), Some("b.md"));
        assert_eq!(diagnostics[0].message, "transitively redundant");
    }

    #[test]
    fn no_diagnostics_when_no_redundancy() {
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
        let diagnostics = RedundantEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
