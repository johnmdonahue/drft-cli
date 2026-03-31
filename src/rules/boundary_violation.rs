use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct BoundaryViolationRule;

impl Rule for BoundaryViolationRule {
    fn name(&self) -> &str {
        "boundary-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.graph_boundaries;

        if !result.sealed {
            return vec![];
        }

        result
            .escapes
            .iter()
            .map(|e| Diagnostic {
                rule: "boundary-violation".into(),
                message: "links outside graph boundary".into(),
                source: Some(e.source.clone()),
                target: Some(e.target.clone()),
                fix: Some(format!(
                    "link reaches outside the graph \u{2014} move {} into the graph or remove the link from {}",
                    e.target, e.source
                )),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::make_enriched_with_root;
    use crate::graph::{Edge, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some(".".into()),
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "../README.md".into(),
            node_type: NodeType::External,
            hash: None,
            graph: Some("..".into()),
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../README.md".into(),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched_with_root(graph, dir.path());
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "boundary-violation");
        assert_eq!(diagnostics[0].target.as_deref(), Some("../README.md"));
    }

    #[test]
    fn detects_deep_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some(".".into()),
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "../../other.md".into(),
            node_type: NodeType::External,
            hash: None,
            graph: Some("..".into()),
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../../other.md".into(),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched_with_root(graph, dir.path());
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn no_violation_for_internal_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched_with_root(graph, dir.path());
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn vacuous_without_lockfile() {
        let dir = TempDir::new().unwrap();

        let mut graph = Graph::new();
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../escape.md".into(),
            link: None,
            parser: "markdown".into(),
        });

        let enriched = make_enriched_with_root(graph, dir.path());
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert!(
            diagnostics.is_empty(),
            "no lockfile means no boundary to enforce"
        );
    }
}
