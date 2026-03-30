use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::graph_boundaries::GraphBoundaries;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

/// See `docs/rules/encapsulation.md` for details.
pub struct EncapsulationRule;

impl Rule for EncapsulationRule {
    fn name(&self) -> &str {
        "encapsulation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = GraphBoundaries.run(&analysis_ctx);

        result
            .encapsulation_violations
            .iter()
            .map(|v| Diagnostic {
                rule: "encapsulation".into(),
                message: format!("not in {}interface", v.scope),
                source: Some(v.source.clone()),
                target: Some(v.target.clone()),
                fix: Some(format!(
                    "{} is not exposed by the {}interface \u{2014} either add it to the interface or remove the link from {}",
                    v.target, v.scope, v.source
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
    use crate::lockfile::{Lockfile, LockfileInterface, LockfileNode, write_lockfile};
    use crate::rules::RuleContext;
    use std::collections::BTreeMap;
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

    fn setup_sealed_child(dir: &Path) {
        let research = dir.join("research");
        fs::create_dir_all(&research).unwrap();
        fs::write(research.join("overview.md"), "# Overview").unwrap();
        fs::write(research.join("internal.md"), "# Internal").unwrap();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "overview.md".into(),
            LockfileNode {
                node_type: NodeType::Source,
                hash: Some("b3:aaa".into()),
                graph: None,
            },
        );
        nodes.insert(
            "internal.md".into(),
            LockfileNode {
                node_type: NodeType::Source,
                hash: Some("b3:bbb".into()),
                graph: None,
            },
        );

        let lockfile = Lockfile {
            lockfile_version: 2,
            interface: Some(LockfileInterface {
                nodes: vec!["overview.md".into()],
            }),
            nodes,
        };
        write_lockfile(&research, &lockfile).unwrap();
    }

    #[test]
    fn no_violation_for_interface_file() {
        let dir = TempDir::new().unwrap();
        setup_sealed_child(dir.path());

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
        });
        graph.add_node(Node {
            path: "research/overview.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: Some("research/".into()),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/overview.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });
        graph.add_edge(Edge {
            source: "research/overview.md".into(),
            target: "research/".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = EncapsulationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn violation_for_non_interface_file() {
        let dir = TempDir::new().unwrap();
        setup_sealed_child(dir.path());

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/internal.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let diagnostics = EncapsulationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "encapsulation");
        assert_eq!(
            diagnostics[0].target.as_deref(),
            Some("research/internal.md")
        );
    }
}
