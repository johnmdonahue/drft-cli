use super::Analysis;
use crate::graph::{EdgeType, Graph, NodeType};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EdgeStatus {
    Valid,
    Broken,
    Excluded,
    DirectoryTarget,
    SymlinkTarget { resolved: String },
    External,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ClassifiedEdge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    pub status: EdgeStatus,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EdgeClassificationResult {
    pub edges: Vec<ClassifiedEdge>,
}

pub struct EdgeClassification;

impl Analysis for EdgeClassification {
    type Output = EdgeClassificationResult;

    fn name(&self) -> &str {
        "edge-classification"
    }

    fn run(&self, graph: &Graph, root: &Path) -> EdgeClassificationResult {
        let edges = graph
            .edges
            .iter()
            .map(|edge| {
                let status = classify_edge(graph, root, &edge.source, &edge.target);
                ClassifiedEdge {
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                    edge_type: edge.edge_type,
                    status,
                }
            })
            .collect();

        EdgeClassificationResult { edges }
    }
}

fn classify_edge(graph: &Graph, root: &Path, _source: &str, target: &str) -> EdgeStatus {
    // External URLs
    if target.starts_with("http://") || target.starts_with("https://") {
        return EdgeStatus::External;
    }

    // Known node in graph — check for special cases
    if let Some(node) = graph.nodes.get(target) {
        // Frontier nodes are valid scope boundaries
        if node.node_type == NodeType::Frontier {
            return EdgeStatus::Valid;
        }
        // Check if the target is actually a symlink on disk
        let target_path = root.join(target);
        if target_path.is_symlink() {
            let resolved = std::fs::read_link(&target_path)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            return EdgeStatus::SymlinkTarget { resolved };
        }
        return EdgeStatus::Valid;
    }

    // Target not in graph — filesystem checks
    let target_path = root.join(target);

    if target_path.is_dir() {
        return EdgeStatus::DirectoryTarget;
    }

    if target_path.is_symlink() {
        let resolved = std::fs::read_link(&target_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        return EdgeStatus::SymlinkTarget { resolved };
    }

    if target_path.exists() {
        // File exists but was excluded by ignore pattern
        return EdgeStatus::Excluded;
    }

    EdgeStatus::Broken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Graph, Node, NodeType};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_valid_edge() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();
        fs::write(dir.path().join("b.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let result = EdgeClassification.run(&graph, dir.path());
        assert_eq!(result.edges.len(), 1);
        assert!(matches!(result.edges[0].status, EdgeStatus::Valid));
    }

    #[test]
    fn classifies_broken_edge() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "gone.md"));

        let result = EdgeClassification.run(&graph, dir.path());
        assert!(matches!(result.edges[0].status, EdgeStatus::Broken));
    }

    #[test]
    fn classifies_external_edge() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(Node {
            path: "https://example.com".into(),
            node_type: NodeType::External,
            hash: None,
        });
        graph.add_edge(make_edge("a.md", "https://example.com"));

        let result = EdgeClassification.run(&graph, Path::new("."));
        assert!(matches!(result.edges[0].status, EdgeStatus::External));
    }

    #[test]
    fn classifies_directory_target() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "").unwrap();
        let guides = dir.path().join("guides");
        fs::create_dir(&guides).unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "guides"));

        let result = EdgeClassification.run(&graph, dir.path());
        assert!(matches!(
            result.edges[0].status,
            EdgeStatus::DirectoryTarget
        ));
    }

    #[test]
    fn classifies_frontier_as_valid() {
        let dir = TempDir::new().unwrap();

        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(Node {
            path: "child/".into(),
            node_type: NodeType::Frontier,
            hash: None,
        });
        graph.add_edge(make_edge("a.md", "child/"));

        let result = EdgeClassification.run(&graph, dir.path());
        assert!(matches!(result.edges[0].status, EdgeStatus::Valid));
    }
}
