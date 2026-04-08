use super::{Analysis, AnalysisContext};
use crate::lockfile::read_lockfile;

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphEscape {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EncapsulationViolation {
    pub source: String,
    pub target: String,
    pub graph: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphBoundariesResult {
    pub sealed: bool,
    pub escapes: Vec<GraphEscape>,
    pub encapsulation_violations: Vec<EncapsulationViolation>,
}

pub struct GraphBoundaries;

impl Analysis for GraphBoundaries {
    type Output = GraphBoundariesResult;

    fn name(&self) -> &str {
        "graph-boundaries"
    }

    fn run(&self, ctx: &AnalysisContext) -> GraphBoundariesResult {
        let graph = ctx.graph;
        let root = ctx.root;
        let sealed = root.join("drft.lock").exists() || root.join("drft.toml").exists();

        // Find graph escapes: nodes with graph: ".."
        let escapes = if sealed {
            graph
                .edges
                .iter()
                .filter(|edge| {
                    graph
                        .nodes
                        .get(&edge.target)
                        .is_some_and(|n| n.graph.as_deref() == Some(".."))
                })
                .map(|edge| GraphEscape {
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // Find encapsulation violations
        let mut encapsulation_violations = Vec::new();

        for (path, node) in &graph.nodes {
            if !node.is_graph {
                continue;
            }

            let child_dir = root.join(path);

            // Try reading interface from child lockfile
            let interface_nodes = if let Ok(Some(lf)) = read_lockfile(&child_dir) {
                match &lf.interface {
                    Some(iface) => iface.files.clone(),
                    None => continue, // No interface = open graph, no violations
                }
            } else {
                // No lockfile — try reading child's drft.toml for interface
                let child_config = crate::config::Config::load(&child_dir);
                match child_config {
                    Ok(config) => match config.interface {
                        Some(iface) => iface.files,
                        None => continue, // No interface = open graph
                    },
                    Err(_) => continue,
                }
            };

            for edge in &graph.edges {
                // Skip sources that aren't local (child-graph coupling edges, etc.)
                if let Some(source_node) = graph.nodes.get(&edge.source)
                    && source_node.graph.as_deref() != Some(".")
                {
                    continue;
                }

                // Check if this edge target belongs to this child graph
                let target_node = match graph.nodes.get(&edge.target) {
                    Some(n) => n,
                    None => continue,
                };
                if target_node.graph.as_deref() != Some(path.as_str()) {
                    continue;
                }

                // Strip the child graph prefix to get the relative path
                let prefix = format!("{path}/");
                let relative_target = match edge.target.strip_prefix(&prefix) {
                    Some(rel) => rel,
                    None => continue,
                };
                if !interface_nodes.iter().any(|n| n == relative_target) {
                    encapsulation_violations.push(EncapsulationViolation {
                        source: edge.source.clone(),
                        target: edge.target.clone(),
                        graph: path.clone(),
                    });
                }
            }
        }

        GraphBoundariesResult {
            sealed,
            escapes,
            encapsulation_violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Graph, Node, NodeType};
    use crate::lockfile::{Lockfile, LockfileInterface, LockfileNode, write_lockfile};
    use std::collections::BTreeMap;
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_ctx<'a>(graph: &'a Graph, root: &'a Path, config: &'a Config) -> AnalysisContext<'a> {
        AnalysisContext {
            graph,
            root,
            config,
            lockfile: None,
        }
    }

    #[test]
    fn detects_graph_escape() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 2\n").unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "../README.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some("..".into()),
            is_graph: false,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_edge(make_edge("index.md", "../README.md"));

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = GraphBoundaries.run(&ctx);
        assert!(result.sealed);
        assert_eq!(result.escapes.len(), 1);
        assert_eq!(result.escapes[0].target, "../README.md");
    }

    #[test]
    fn no_escape_without_lockfile() {
        let dir = TempDir::new().unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_edge(make_edge("index.md", "../README.md"));

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = GraphBoundaries.run(&ctx);
        assert!(!result.sealed);
        assert!(result.escapes.is_empty());
    }

    #[test]
    fn detects_encapsulation_violation() {
        let dir = TempDir::new().unwrap();
        let research = dir.path().join("research");
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
        let lockfile = Lockfile {
            lockfile_version: 2,
            interface: Some(LockfileInterface {
                files: vec!["overview.md".into()],
            }),
            nodes,
        };
        write_lockfile(&research, &lockfile).unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "research".into(),
            node_type: NodeType::Directory,
            hash: None,
            graph: Some(".".into()),
            is_graph: true,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_node(Node {
            path: "research/internal.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some("research".into()),
            is_graph: false,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_edge(make_edge("index.md", "research/internal.md"));

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = GraphBoundaries.run(&ctx);
        assert_eq!(result.encapsulation_violations.len(), 1);
        assert_eq!(
            result.encapsulation_violations[0].target,
            "research/internal.md"
        );
        assert_eq!(result.encapsulation_violations[0].graph, "research");
    }

    #[test]
    fn interface_file_is_not_violation() {
        let dir = TempDir::new().unwrap();
        let research = dir.path().join("research");
        fs::create_dir_all(&research).unwrap();
        fs::write(research.join("overview.md"), "# Overview").unwrap();

        let mut nodes = BTreeMap::new();
        nodes.insert(
            "overview.md".into(),
            LockfileNode {
                node_type: NodeType::File,
                hash: Some("b3:aaa".into()),
                graph: None,
            },
        );
        let lockfile = Lockfile {
            lockfile_version: 2,
            interface: Some(LockfileInterface {
                files: vec!["overview.md".into()],
            }),
            nodes,
        };
        write_lockfile(&research, &lockfile).unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("index.md"));
        graph.add_node(Node {
            path: "research".into(),
            node_type: NodeType::Directory,
            hash: None,
            graph: Some(".".into()),
            is_graph: true,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_node(Node {
            path: "research/overview.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: Some("research".into()),
            is_graph: false,
            metadata: HashMap::new(),
            included: false,
        });
        graph.add_edge(make_edge("index.md", "research/overview.md"));

        let config = Config::defaults();
        let ctx = make_ctx(&graph, dir.path(), &config);
        let result = GraphBoundaries.run(&ctx);
        assert!(result.encapsulation_violations.is_empty());
    }
}
