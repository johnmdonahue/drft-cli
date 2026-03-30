use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::connected_components::ConnectedComponents;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

/// See `docs/rules/fragmentation.md` for details.
pub struct FragmentationRule;

impl Rule for FragmentationRule {
    fn name(&self) -> &str {
        "fragmentation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = ConnectedComponents.run(&analysis_ctx);

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
                    node: Some(members.clone()),
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
    use crate::config::Config;
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::rules::RuleContext;
    use std::path::Path;

    fn make_ctx<'a>(graph: &'a Graph, config: &'a Config) -> RuleContext<'a> {
        RuleContext {
            graph,
            root: Path::new("."),
            config,
            lockfile: None,
        }
    }

    #[test]
    fn no_diagnostic_when_connected() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let config = Config::defaults();
        let diagnostics = FragmentationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_fragmentation() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let config = Config::defaults();
        let diagnostics = FragmentationRule.evaluate(&make_ctx(&graph, &config));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "fragmentation");
        assert!(diagnostics[0].message.contains("disconnected component"));
    }

    #[test]
    fn no_diagnostic_for_empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let diagnostics = FragmentationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }
}
