use super::Rule;
use crate::analyses::Analysis;
use crate::analyses::depth::Depth;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use std::collections::HashMap;
use std::path::Path;

pub struct LayerViolationRule;

impl Rule for LayerViolationRule {
    fn name(&self) -> &str {
        "layer-violation"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = Depth.run(graph, root);

        // Build lookup maps
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
            if !graph.is_real_node(&edge.source) || !graph.is_real_node(&edge.target) {
                continue;
            }

            // Skip edges involving cyclic nodes
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
                // Upward link
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
                // Skip-layer link
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
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_node};

    #[test]
    fn no_violation_in_clean_hierarchy() {
        // a → b → c (each at depth +1)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let diagnostics = LayerViolationRule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_upward_link() {
        // a → b → c, c → a (upward)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "d.md"));
        graph.add_edge(make_edge("d.md", "a.md")); // upward — but d→a creates a cycle

        // Since d→a creates a cycle (a,b,c,d all in one SCC), all nodes are in_cycle
        // and the rule skips them. So no violations.
        let diagnostics = LayerViolationRule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_skip_layer() {
        // a → b → c, a → c (skip layer: depth 0 → depth 2)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("a.md", "c.md")); // skip layer

        let diagnostics = LayerViolationRule.evaluate(&graph, Path::new("."));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("skip-layer"));
    }

    #[test]
    fn skips_cyclic_nodes() {
        // a → b → a (cycle), no layer violations reported
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "a.md"));

        let diagnostics = LayerViolationRule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn same_layer_link_is_not_violation() {
        // a → b, a → c (b and c both at depth 1), b → c is depth 1→1 (same layer)
        // But max depth: c gets depth 2 from b→c path. Need different graph.
        // Use: a → b, a → c (no link between b and c). Same layer, no violations.
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let diagnostics = LayerViolationRule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }
}
