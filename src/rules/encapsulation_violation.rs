use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct EncapsulationViolationRule;

impl Rule for EncapsulationViolationRule {
    fn name(&self) -> &str {
        "encapsulation-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let result = &ctx.graph.graph_boundaries;

        result
            .encapsulation_violations
            .iter()
            .map(|v| Diagnostic {
                rule: "encapsulation-violation".into(),
                message: format!("not in {}interface", v.graph),
                source: Some(v.source.clone()),
                target: Some(v.target.clone()),
                fix: Some(format!(
                    "{} is not exposed by the {}interface \u{2014} either add it to the interface or remove the link from {}",
                    v.target, v.graph, v.source
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
    use crate::graph::{Edge, Graph, Node, NodeType};
    use crate::lockfile::{Lockfile, LockfileInterface, LockfileNode, write_lockfile};
    use crate::rules::RuleContext;
    use std::collections::{BTreeMap, HashMap};
    use std::fs;
    use tempfile::TempDir;

    fn make_enriched(graph: Graph, root: &std::path::Path) -> crate::analyses::EnrichedGraph {
        crate::analyses::enrich_graph(graph, root, &Config::defaults(), None)
    }

    fn setup_sealed_child(dir: &std::path::Path) {
        let research = dir.join("research");
        fs::create_dir_all(&research).unwrap();
        fs::write(research.join("overview.md"), "# Overview").unwrap();
        fs::write(research.join("internal.md"), "# Internal").unwrap();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "overview.md".into(),
            LockfileNode {
                node_type: NodeType::File,
                hash: Some("b3:aaa".into()),
                graph: None,
            },
        );
        nodes.insert(
            "internal.md".into(),
            LockfileNode {
                node_type: NodeType::File,
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
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "research/overview.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some("research/".into()),
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/overview.md".into(),
            link: None, parser: "markdown".into(),
        });
        graph.add_edge(Edge {
            source: "research/overview.md".into(),
            target: "research/".into(),
            link: None, parser: "markdown".into(),
        });

        let enriched = make_enriched(graph, dir.path());
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = EncapsulationViolationRule.evaluate(&ctx);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn violation_for_non_interface_file() {
        let dir = TempDir::new().unwrap();
        setup_sealed_child(dir.path());

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_node(Node {
            path: "research/".into(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "research/internal.md".into(),
            link: None, parser: "markdown".into(),
        });

        let enriched = make_enriched(graph, dir.path());
        let ctx = RuleContext { graph: &enriched, options: None };
        let diagnostics = EncapsulationViolationRule.evaluate(&ctx);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "encapsulation-violation");
        assert_eq!(
            diagnostics[0].target.as_deref(),
            Some("research/internal.md")
        );
    }
}
