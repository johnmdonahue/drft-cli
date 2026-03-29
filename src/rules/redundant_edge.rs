use super::Rule;
use crate::analysis::Analysis;
use crate::analysis::transitive_reduction::TransitiveReduction;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use std::path::Path;

pub struct RedundantEdgeRule;

impl Rule for RedundantEdgeRule {
    fn name(&self) -> &str {
        "redundant-edge"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let analysis = TransitiveReduction;
        let result = analysis.run(graph, root);

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
    use crate::graph::{Edge, EdgeType, Node, NodeType};

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::Document,
            hash: None,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            edge_type: EdgeType::Inline,
        }
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

        let rule = RedundantEdgeRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

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

        let rule = RedundantEdgeRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

        assert!(diagnostics.is_empty());
    }
}
