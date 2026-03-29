use super::{Analysis, Metric, MetricKind};
use crate::graph::Graph;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodePageRank {
    pub node: String,
    pub score: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PageRankResult {
    pub iterations: usize,
    pub converged: bool,
    pub nodes: Vec<NodePageRank>,
}

pub struct PageRank;

const DAMPING: f64 = 0.85;
const MAX_ITERATIONS: usize = 100;
const EPSILON: f64 = 1e-6;

impl Analysis for PageRank {
    type Output = PageRankResult;

    fn name(&self) -> &str {
        "pagerank"
    }

    fn run(&self, graph: &Graph, _root: &Path) -> PageRankResult {
        let real_nodes: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|p| graph.is_real_node(p))
            .map(|s| s.as_str())
            .collect();

        let n = real_nodes.len();
        if n == 0 {
            return PageRankResult {
                iterations: 0,
                converged: true,
                nodes: Vec::new(),
            };
        }

        // Build adjacency among real nodes only
        let mut out_degree: HashMap<&str, usize> = HashMap::new();
        let mut inbound: HashMap<&str, Vec<&str>> = HashMap::new();

        for &node in &real_nodes {
            out_degree.insert(node, 0);
            inbound.insert(node, Vec::new());
        }

        for edge in &graph.edges {
            if graph.is_real_node(&edge.source)
                && graph.is_real_node(&edge.target)
                && edge.source != edge.target
            {
                *out_degree.get_mut(edge.source.as_str()).unwrap() += 1;
                inbound
                    .get_mut(edge.target.as_str())
                    .unwrap()
                    .push(edge.source.as_str());
            }
        }

        let init = 1.0 / n as f64;
        let mut rank: HashMap<&str, f64> = HashMap::new();
        for &node in &real_nodes {
            rank.insert(node, init);
        }

        // Identify dangling nodes (out-degree 0)
        let dangling: Vec<&str> = real_nodes
            .iter()
            .filter(|&&node| out_degree[node] == 0)
            .copied()
            .collect();

        let mut iterations = 0;
        let mut converged = false;

        for _ in 0..MAX_ITERATIONS {
            iterations += 1;

            // Dangling node contribution distributed evenly
            let dangling_sum: f64 = dangling.iter().map(|&node| rank[node]).sum();

            let mut new_rank: HashMap<&str, f64> = HashMap::new();
            let base = (1.0 - DAMPING) / n as f64 + DAMPING * dangling_sum / n as f64;

            for &node in &real_nodes {
                let mut incoming_sum = 0.0;
                for &pred in &inbound[node] {
                    incoming_sum += rank[pred] / out_degree[pred] as f64;
                }
                new_rank.insert(node, base + DAMPING * incoming_sum);
            }

            // Check convergence (L1 norm)
            let diff: f64 = real_nodes
                .iter()
                .map(|&node| (new_rank[node] - rank[node]).abs())
                .sum();

            rank = new_rank;

            if diff < EPSILON {
                converged = true;
                break;
            }
        }

        let mut nodes: Vec<NodePageRank> = rank
            .into_iter()
            .map(|(node, score)| NodePageRank {
                node: node.to_string(),
                score,
            })
            .collect();

        // Sort by score descending, then node ascending
        nodes.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap()
                .then_with(|| a.node.cmp(&b.node))
        });

        PageRankResult {
            iterations,
            converged,
            nodes,
        }
    }

    fn metrics(&self, output: &PageRankResult, _graph: &Graph) -> Vec<Metric> {
        if output.nodes.is_empty() {
            return vec![];
        }
        let max = output.nodes.iter().map(|n| n.score).fold(0.0f64, f64::max);
        let scores: Vec<f64> = output.nodes.iter().map(|n| n.score).collect();
        let gini = gini_coefficient(&scores);

        vec![
            Metric {
                name: "max_pagerank".into(),
                value: max,
                kind: MetricKind::Score,
                dimension: "consistency".into(),
            },
            Metric {
                name: "pagerank_gini".into(),
                value: gini,
                kind: MetricKind::Ratio,
                dimension: "consistency".into(),
            },
        ]
    }
}

fn gini_coefficient(values: &[f64]) -> f64 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum: f64 = sorted.iter().sum();
    if sum == 0.0 {
        return 0.0;
    }
    let mut numerator = 0.0;
    for (i, &v) in sorted.iter().enumerate() {
        numerator += (2.0 * (i + 1) as f64 - n as f64 - 1.0) * v;
    }
    numerator / (n as f64 * sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;
    use crate::graph::test_helpers::{make_edge, make_node};

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let result = PageRank.run(&graph, Path::new("."));
        assert!(result.converged);
        assert_eq!(result.nodes.len(), 1);
        assert!((result.nodes[0].score - 1.0).abs() < 1e-4);
    }

    #[test]
    fn two_nodes_with_link() {
        // a → b: b should have higher rank
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let result = PageRank.run(&graph, Path::new("."));
        assert!(result.converged);
        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        let b = result.nodes.iter().find(|n| n.node == "b.md").unwrap();
        assert!(b.score > a.score);
    }

    #[test]
    fn scores_sum_to_one() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let result = PageRank.run(&graph, Path::new("."));
        let sum: f64 = result.nodes.iter().map(|n| n.score).sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let result = PageRank.run(&graph, Path::new("."));
        assert!(result.converged);
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn dangling_node() {
        // a → b, c is dangling (out-degree 0)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let result = PageRank.run(&graph, Path::new("."));
        assert!(result.converged);
        let sum: f64 = result.nodes.iter().map(|n| n.score).sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }
}
