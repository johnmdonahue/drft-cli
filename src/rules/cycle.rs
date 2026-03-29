use crate::analysis::Analysis;
use crate::analysis::scc::StronglyConnectedComponents;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct CycleRule;

impl Rule for CycleRule {
    fn name(&self) -> &str {
        "cycle"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = StronglyConnectedComponents.run(graph, root);

        result
            .sccs
            .iter()
            .map(|scc| {
                // Build a cycle path: members + repeat first to close the cycle
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
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_node};

    #[test]
    fn detects_simple_cycle() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "cycle");

        let path = diagnostics[0].path.as_ref().unwrap();
        // Cycle should start and end with the same node
        assert_eq!(path.first(), path.last());
        // All three nodes should be in the path
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

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_broken_link_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        // Edge to non-existent node should not crash or produce false cycle
        graph.add_edge(make_edge("a.md", "missing.md"));

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }
}
