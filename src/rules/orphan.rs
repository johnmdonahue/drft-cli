use crate::analysis::Analysis;
use crate::analysis::degree::Degree;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct OrphanRule;

impl Rule for OrphanRule {
    fn name(&self) -> &str {
        "orphan"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        let result = Degree.run(graph, root);

        result
            .nodes
            .iter()
            .filter(|nd| nd.in_degree == 0)
            .map(|nd| Diagnostic {
                rule: "orphan".into(),
                message: "no inbound links".into(),
                node: Some(nd.node.clone()),
                fix: Some(format!(
                    "{} has no inbound links — either link to it from another file or remove it",
                    nd.node
                )),
                ..Default::default()
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
    fn detects_orphan() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("orphan.md"));
        graph.add_edge(make_edge("index.md", "setup.md"));

        let rule = OrphanRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        assert!(orphan_nodes.contains(&"orphan.md"));
        // index.md is also an orphan (nothing links to it), which is expected
        assert!(orphan_nodes.contains(&"index.md"));
    }

    #[test]
    fn linked_file_is_not_orphan() {
        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(make_node("setup.md"));
        graph.add_edge(make_edge("index.md", "setup.md"));

        let rule = OrphanRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));

        let orphan_nodes: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.node.as_deref().unwrap())
            .collect();
        // setup.md has an inbound link — not an orphan
        assert!(!orphan_nodes.contains(&"setup.md"));
        // index.md has no inbound links — is an orphan
        assert!(orphan_nodes.contains(&"index.md"));
    }
}
