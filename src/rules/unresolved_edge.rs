use crate::diagnostic::Diagnostic;
use crate::graph::{Resolution, TargetKind};
use crate::rules::{Rule, RuleContext};

pub struct UnresolvedEdgeRule;

impl Rule for UnresolvedEdgeRule {
    fn name(&self) -> &str {
        "unresolved-edge"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;

        graph
            .edges
            .iter()
            .filter_map(|edge| {
                if !matches!(edge.target_kind, TargetKind::Internal(Resolution::Missing)) {
                    return None;
                }

                Some(Diagnostic {
                    rule: "unresolved-edge".into(),
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
    use crate::graph::test_helpers::{make_edge, make_enriched, make_node};
    use crate::graph::{Edge, Graph, Location, TargetKind};
    use crate::rules::RuleContext;

    #[test]
    fn detects_unresolved_edge() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            target_kind: TargetKind::Internal(Resolution::Missing),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = UnresolvedEdgeRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "unresolved-edge");
        assert_eq!(diagnostics[0].source.as_deref(), Some("index.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("gone.md"));
    }

    #[test]
    fn no_diagnostic_for_found_target() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("setup.md"));
        graph.add_edge(make_edge("index.md", "setup.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = UnresolvedEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn no_diagnostic_for_external_targets() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "assets/logo.png".into(),
            target_kind: TargetKind::External(Location::Local),
            link: None,
            parser: "markdown".into(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "https://example.com".into(),
            target_kind: TargetKind::External(Location::Remote),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = UnresolvedEdgeRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
