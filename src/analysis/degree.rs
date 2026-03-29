use super::{Analysis, Metric, MetricKind};
use crate::graph::Graph;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeDegree {
    pub node: String,
    pub in_degree: usize,
    pub out_degree: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DegreeResult {
    pub nodes: Vec<NodeDegree>,
}

pub struct Degree;

impl Analysis for Degree {
    type Output = DegreeResult;

    fn name(&self) -> &str {
        "degree"
    }

    fn run(&self, graph: &Graph, _root: &Path) -> DegreeResult {
        let mut nodes: Vec<NodeDegree> = graph
            .nodes
            .keys()
            .filter(|path| graph.is_real_node(path))
            .map(|path| {
                let in_degree = graph
                    .reverse
                    .get(path.as_str())
                    .map(|indices| {
                        indices
                            .iter()
                            .filter(|&&idx| graph.is_real_node(&graph.edges[idx].source))
                            .count()
                    })
                    .unwrap_or(0);

                let out_degree = graph
                    .forward
                    .get(path.as_str())
                    .map(|indices| {
                        indices
                            .iter()
                            .filter(|&&idx| graph.is_real_node(&graph.edges[idx].target))
                            .count()
                    })
                    .unwrap_or(0);

                NodeDegree {
                    node: path.clone(),
                    in_degree,
                    out_degree,
                }
            })
            .collect();

        nodes.sort_by(|a, b| a.node.cmp(&b.node));

        DegreeResult { nodes }
    }

    fn metrics(&self, output: &DegreeResult, _graph: &Graph) -> Vec<Metric> {
        let total = output.nodes.len() as f64;
        if total == 0.0 {
            return vec![];
        }
        let orphans = output.nodes.iter().filter(|n| n.in_degree == 0).count() as f64;
        let islands = output
            .nodes
            .iter()
            .filter(|n| n.in_degree == 0 && n.out_degree == 0)
            .count() as f64;
        let sinks = output.nodes.iter().filter(|n| n.out_degree == 0).count() as f64;

        vec![
            Metric {
                name: "orphan_ratio".into(),
                value: orphans / total,
                kind: MetricKind::Ratio,
                dimension: "completeness".into(),
            },
            Metric {
                name: "island_ratio".into(),
                value: islands / total,
                kind: MetricKind::Ratio,
                dimension: "completeness".into(),
            },
            Metric {
                name: "sink_ratio".into(),
                value: sinks / total,
                kind: MetricKind::Ratio,
                dimension: "completeness".into(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Graph, Node, NodeType};

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let result = Degree.run(&graph, Path::new("."));
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let result = Degree.run(&graph, Path::new("."));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].node, "a.md");
        assert_eq!(result.nodes[0].in_degree, 0);
        assert_eq!(result.nodes[0].out_degree, 0);
    }

    #[test]
    fn diamond_graph() {
        // a → b, a → c, b → d, c → d
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));
        graph.add_edge(make_edge("b.md", "d.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let result = Degree.run(&graph, Path::new("."));
        assert_eq!(result.nodes.len(), 4);

        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        assert_eq!(a.in_degree, 0);
        assert_eq!(a.out_degree, 2);

        let d = result.nodes.iter().find(|n| n.node == "d.md").unwrap();
        assert_eq!(d.in_degree, 2);
        assert_eq!(d.out_degree, 0);
    }

    #[test]
    fn self_loop() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "a.md"));

        let result = Degree.run(&graph, Path::new("."));
        assert_eq!(result.nodes[0].in_degree, 1);
        assert_eq!(result.nodes[0].out_degree, 1);
    }

    #[test]
    fn excludes_synthetic_nodes() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(Node {
            path: "https://example.com".into(),
            node_type: NodeType::External,
            hash: None,
        });
        graph.add_edge(make_edge("a.md", "https://example.com"));

        let result = Degree.run(&graph, Path::new("."));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].node, "a.md");
        // Edge to external node is not counted
        assert_eq!(result.nodes[0].out_degree, 0);
    }

    #[test]
    fn sorted_by_path() {
        let mut graph = Graph::new();
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));

        let result = Degree.run(&graph, Path::new("."));
        let paths: Vec<&str> = result.nodes.iter().map(|n| n.node.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md", "c.md"]);
    }
}
