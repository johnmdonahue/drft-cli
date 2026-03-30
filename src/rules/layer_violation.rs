use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::depth::Depth;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};
use std::collections::HashMap;

pub struct LayerViolationRule;

impl Rule for LayerViolationRule {
    fn name(&self) -> &str {
        "layer-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = ctx.graph;
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = Depth.run(&analysis_ctx);

        let depth_map: HashMap<&str, usize> = result
            .nodes
            .iter()
            .map(|n| (n.node.as_str(), n.depth))
            .collect();
        let cycle_map: HashMap<&str, bool> = result
            .nodes
            .iter()
            .map(|n| (n.node.as_str(), n.in_cycle))
            .collect();

        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            if !graph.is_file_node(&edge.source) || !graph.is_file_node(&edge.target) {
                continue;
            }

            if cycle_map.get(edge.source.as_str()) == Some(&true)
                || cycle_map.get(edge.target.as_str()) == Some(&true)
            {
                continue;
            }

            let Some(&src_depth) = depth_map.get(edge.source.as_str()) else {
                continue;
            };
            let Some(&tgt_depth) = depth_map.get(edge.target.as_str()) else {
                continue;
            };

            if tgt_depth < src_depth {
                diagnostics.push(Diagnostic {
                    rule: "layer-violation".into(),
                    message: format!(
                        "upward link (depth {} \u{2192} depth {})",
                        src_depth, tgt_depth
                    ),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} (depth {}) links to {} (depth {}) \u{2014} this points upward in the hierarchy",
                        edge.source, src_depth, edge.target, tgt_depth
                    )),
                    ..Default::default()
                });
            } else if tgt_depth > src_depth + 1 {
                diagnostics.push(Diagnostic {
                    rule: "layer-violation".into(),
                    message: format!(
                        "skip-layer link (depth {} \u{2192} depth {})",
                        src_depth, tgt_depth
                    ),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "{} (depth {}) links to {} (depth {}), skipping {} layers",
                        edge.source,
                        src_depth,
                        edge.target,
                        tgt_depth,
                        tgt_depth - src_depth - 1
                    )),
                    ..Default::default()
                });
            }
        }

        diagnostics
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
    fn no_violation_in_clean_hierarchy() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = LayerViolationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_upward_link() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "d.md"));
        graph.add_edge(make_edge("d.md", "a.md"));

        let config = Config::defaults();
        let diagnostics = LayerViolationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_skip_layer() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = LayerViolationRule.evaluate(&make_ctx(&graph, &config));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("skip-layer"));
    }

    #[test]
    fn skips_cyclic_nodes() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "a.md"));

        let config = Config::defaults();
        let diagnostics = LayerViolationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn same_layer_link_is_not_violation() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = LayerViolationRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }
}
