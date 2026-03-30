use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::graph_boundaries::GraphBoundaries;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct BoundaryViolationRule;

impl Rule for BoundaryViolationRule {
    fn name(&self) -> &str {
        "boundary-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = GraphBoundaries.run(&analysis_ctx);

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
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_ctx<'a>(graph: &'a Graph, root: &'a Path, config: &'a Config) -> RuleContext<'a> {
        RuleContext {
            graph,
            root,
            config,
            lockfile: None,
        }
    }

    #[test]
    fn detects_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../README.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
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
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../../other.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
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
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
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
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = BoundaryViolationRule.evaluate(&ctx);
        assert!(
            diagnostics.is_empty(),
            "no lockfile means no boundary to enforce"
        );
    }
}
