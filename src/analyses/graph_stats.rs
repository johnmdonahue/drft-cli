use super::{Analysis, AnalysisContext};
use crate::graph::Graph;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct GraphStatsResult {
    pub node_count: usize,
    pub edge_count: usize,
    pub density: f64,
    pub diameter: Option<usize>,
    pub average_path_length: Option<f64>,
}

pub struct GraphStats;

impl Analysis for GraphStats {
    type Output = GraphStatsResult;

    fn name(&self) -> &str {
        "graph-stats"
    }

    fn run(&self, ctx: &AnalysisContext) -> GraphStatsResult {
        let graph = ctx.graph;
        let real_nodes: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|p| graph.is_file_node(p))
            .map(|s| s.as_str())
            .collect();

        let node_count = real_nodes.len();

        // Count edges between real nodes
        let edge_count = graph
            .edges
            .iter()
            .filter(|e| graph.is_internal_edge(e))
            .count();

        // Density for directed graph: |E| / (|V| * (|V| - 1))
        let density = if node_count <= 1 {
            0.0
        } else {
            edge_count as f64 / (node_count * (node_count - 1)) as f64
        };

        // All-pairs shortest paths via BFS from each real node (directed)
        let (diameter, average_path_length) = if node_count <= 1 {
            (Some(0), Some(0.0))
        } else {
            all_pairs_stats(graph, &real_nodes)
        };

        GraphStatsResult {
            node_count,
            edge_count,
            density,
            diameter,
            average_path_length,
        }
    }
}

/// Compute diameter and average path length via BFS from each node.
/// Returns (None, None) if the directed graph is not strongly connected.
fn all_pairs_stats(graph: &Graph, real_nodes: &[&str]) -> (Option<usize>, Option<f64>) {
    let mut max_dist: usize = 0;
    let mut total_dist: u64 = 0;
    let mut pair_count: u64 = 0;
    let n = real_nodes.len();

    for &source in real_nodes {
        let distances = bfs_distances(graph, source);

        for &target in real_nodes {
            if source == target {
                continue;
            }
            match distances.get(target) {
                Some(&d) => {
                    max_dist = max_dist.max(d);
                    total_dist += d as u64;
                    pair_count += 1;
                }
                None => {
                    // Not reachable — graph is not strongly connected
                    return (None, None);
                }
            }
        }
    }

    let expected_pairs = (n * (n - 1)) as u64;
    if pair_count < expected_pairs {
        return (None, None);
    }

    let avg = if pair_count > 0 {
        total_dist as f64 / pair_count as f64
    } else {
        0.0
    };

    (Some(max_dist), Some(avg))
}

/// BFS from source, returning distances to all reachable real nodes.
fn bfs_distances<'a>(graph: &'a Graph, source: &'a str) -> HashMap<&'a str, usize> {
    let mut distances = HashMap::new();
    let mut queue = VecDeque::new();

    distances.insert(source, 0);
    queue.push_back(source);

    while let Some(current) = queue.pop_front() {
        let current_dist = distances[current];
        if let Some(edge_indices) = graph.forward.get(current) {
            for &idx in edge_indices {
                let edge = &graph.edges[idx];
                let target = edge.target.as_str();
                if graph.is_internal_edge(edge) && !distances.contains_key(target) {
                    distances.insert(target, current_dist + 1);
                    queue.push_back(target);
                }
            }
        }
    }

    distances
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::test_helpers::{make_edge, make_node};
    use std::path::Path;

    fn make_ctx<'a>(graph: &'a Graph, config: &'a Config) -> AnalysisContext<'a> {
        AnalysisContext {
            graph,
            root: Path::new("."),
            config,
            lockfile: None,
        }
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = GraphStats.run(&make_ctx(&graph, &config));
        assert_eq!(result.node_count, 0);
        assert_eq!(result.edge_count, 0);
        assert_eq!(result.density, 0.0);
        assert_eq!(result.diameter, Some(0));
    }

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        let config = Config::defaults();
        let result = GraphStats.run(&make_ctx(&graph, &config));
        assert_eq!(result.node_count, 1);
        assert_eq!(result.density, 0.0);
        assert_eq!(result.diameter, Some(0));
    }

    #[test]
    fn linear_chain() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = GraphStats.run(&make_ctx(&graph, &config));
        assert_eq!(result.node_count, 3);
        assert_eq!(result.edge_count, 2);
        assert!((result.density - 1.0 / 3.0).abs() < 1e-10);
        assert_eq!(result.diameter, None);
    }

    #[test]
    fn complete_bidirectional() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "a.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = GraphStats.run(&make_ctx(&graph, &config));
        assert_eq!(result.node_count, 3);
        assert_eq!(result.edge_count, 6);
        assert!((result.density - 1.0).abs() < 1e-10);
        assert_eq!(result.diameter, Some(1));
        assert_eq!(result.average_path_length, Some(1.0));
    }

    #[test]
    fn cycle_has_diameter() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = GraphStats.run(&make_ctx(&graph, &config));
        assert_eq!(result.diameter, Some(2));
        assert!((result.average_path_length.unwrap() - 1.5).abs() < 1e-10);
    }
}
