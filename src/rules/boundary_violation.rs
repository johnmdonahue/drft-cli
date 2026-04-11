use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct BoundaryViolationRule;

impl Rule for BoundaryViolationRule {
    fn name(&self) -> &str {
        "boundary-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;
        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            if !is_escape(&edge.target) {
                continue;
            }
            diagnostics.push(Diagnostic {
                rule: "boundary-violation".into(),
                message: "link escapes the graph root".into(),
                source: Some(edge.source.clone()),
                target: Some(edge.target.clone()),
                fix: Some(format!(
                    "{} links to {} which resolves outside the drft.toml root \u{2014} move the target into the graph or drop the link",
                    edge.source, edge.target
                )),
                ..Default::default()
            });
        }

        diagnostics.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });
        diagnostics
    }
}

/// Lexical escape: a path whose resolved identity sits above the graph root.
/// Matches `..`, any `../...`, and any absolute path from a parser that emits one.
fn is_escape(target: &str) -> bool {
    target == ".." || target.starts_with("../") || target.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::{make_enriched, make_node};
    use crate::graph::{Edge, Graph};
    use crate::rules::RuleContext;

    fn edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            link: None,
            parser: "markdown".into(),
        }
    }

    #[test]
    fn detects_parent_escape() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("../outside.md"));
        graph.add_edge(edge("index.md", "../outside.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "boundary-violation");
        assert_eq!(diagnostics[0].source.as_deref(), Some("index.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("../outside.md"));
    }

    #[test]
    fn detects_absolute_path() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("/etc/hosts"));
        graph.add_edge(edge("index.md", "/etc/hosts"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].target.as_deref(), Some("/etc/hosts"));
    }

    #[test]
    fn detects_nested_escape() {
        let mut graph = Graph::new();
        graph.add_node(make_node("sub/doc.md"));
        graph.add_node(make_node("../../way/outside.md"));
        graph.add_edge(edge("sub/doc.md", "../../way/outside.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn no_diagnostic_for_in_graph_target() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("setup.md"));
        graph.add_edge(edge("index.md", "setup.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn no_diagnostic_for_subdirectory_target() {
        // Subdirectory targets stay inside the graph root.
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("guides/setup.md"));
        graph.add_edge(edge("index.md", "guides/setup.md"));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }
}
