use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::change_propagation::ChangePropagation;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct StaleRule;

impl Rule for StaleRule {
    fn name(&self) -> &str {
        "stale"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = ChangePropagation.run(&analysis_ctx);

        if !result.has_lockfile {
            return vec![];
        }

        let mut diagnostics = Vec::new();

        for change in &result.directly_changed {
            diagnostics.push(Diagnostic {
                rule: "stale".into(),
                message: "content changed".into(),
                node: Some(change.node.clone()),
                fix: Some(format!(
                    "{} has been modified since the last lock \u{2014} review its dependents, then run drft lock",
                    change.node
                )),
                ..Default::default()
            });
        }

        for stale in &result.transitively_stale {
            diagnostics.push(Diagnostic {
                rule: "stale".into(),
                message: "stale via".into(),
                node: Some(stale.node.clone()),
                via: Some(stale.via.clone()),
                fix: Some(format!(
                    "{} has changed \u{2014} review {} to ensure it still accurately reflects {}, then run drft lock",
                    stale.via, stale.node, stale.via
                )),
                ..Default::default()
            });
        }

        for change in &result.boundary_changes {
            diagnostics.push(Diagnostic {
                rule: "stale".into(),
                message: "graph boundary changed".into(),
                node: Some(change.node.clone()),
                fix: Some(match change.reason.as_str() {
                    "child graph removed" => format!(
                        "{} no longer has a drft.lock \u{2014} run drft lock to update the parent lockfile",
                        change.node
                    ),
                    "new child graph" => format!(
                        "{} is a new child graph \u{2014} run drft lock to update the parent lockfile",
                        change.node
                    ),
                    _ => "run drft lock to update the lockfile".to_string(),
                }),
                ..Default::default()
            });
        }

        diagnostics.sort_by(|a, b| a.node.cmp(&b.node));
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType, hash_bytes};
    use crate::lockfile::{Lockfile, write_lockfile};
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

    fn setup_locked_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
        fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

        let mut graph = Graph::new();
        let index_hash = hash_bytes(b"[setup](setup.md)");
        let setup_hash = hash_bytes(b"# Setup");

        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: Some(index_hash),
            graph: None,
        });
        graph.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: Some(setup_hash),
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let lockfile = Lockfile::from_graph(&graph);
        write_lockfile(dir.path(), &lockfile).unwrap();
        dir
    }

    #[test]
    fn no_staleness_when_unchanged() {
        let dir = setup_locked_dir();
        let graph = Graph::new();
        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = StaleRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_direct_and_transitive_staleness() {
        let dir = setup_locked_dir();
        fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

        let config = Config::defaults();
        let graph = crate::graph::build_graph(dir.path(), &config).unwrap();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = StaleRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 2);

        let direct = diagnostics
            .iter()
            .find(|d| d.message == "content changed")
            .unwrap();
        assert_eq!(direct.node.as_deref(), Some("setup.md"));
        assert!(direct.via.is_none());

        let transitive = diagnostics
            .iter()
            .find(|d| d.message == "stale via")
            .unwrap();
        assert_eq!(transitive.node.as_deref(), Some("index.md"));
        assert_eq!(transitive.via.as_deref(), Some("setup.md"));
    }

    #[test]
    fn skips_when_no_lockfile() {
        let dir = TempDir::new().unwrap();
        let graph = Graph::new();
        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = StaleRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn deleted_file_causes_staleness() {
        let dir = setup_locked_dir();
        fs::remove_file(dir.path().join("setup.md")).unwrap();

        let config = Config::defaults();
        let graph = crate::graph::build_graph(dir.path(), &config).unwrap();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = StaleRule.evaluate(&ctx);
        assert!(diagnostics.len() >= 1);

        let direct = diagnostics
            .iter()
            .find(|d| d.message == "content changed")
            .unwrap();
        assert_eq!(direct.node.as_deref(), Some("setup.md"));
    }
}
