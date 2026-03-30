use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::scc::StronglyConnectedComponents;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct CycleRule;

impl Rule for CycleRule {
    fn name(&self) -> &str {
        "cycle"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = StronglyConnectedComponents.run(&analysis_ctx);

        result
            .sccs
            .iter()
            .map(|scc| {
                let mut path = scc.members.clone();
                if let Some(first) = path.first().cloned() {
                    path.push(first);
                }

                let fix = format!(
                    "circular dependency \u{2014} review whether one of these links can be removed or the content restructured: {}",
                    scc.members.join(" \u{2192} ")
                );

                Diagnostic {
                    rule: "cycle".into(),
                    message: "cycle detected".into(),
                    path: Some(path),
                    fix: Some(fix),
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
    fn detects_simple_cycle() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let diagnostics = CycleRule.evaluate(&make_ctx(&graph, &config));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "cycle");

        let path = diagnostics[0].path.as_ref().unwrap();
        assert_eq!(path.first(), path.last());
        assert!(path.contains(&"a.md".to_string()));
        assert!(path.contains(&"b.md".to_string()));
        assert!(path.contains(&"c.md".to_string()));
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = CycleRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_broken_link_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "missing.md"));

        let config = Config::defaults();
        let diagnostics = CycleRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }
}
