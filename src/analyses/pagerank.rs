use super::{Analysis, AnalysisContext};
use std::collections::HashMap;

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

    fn run(&self, ctx: &AnalysisContext) -> PageRankResult {
        let graph = ctx.graph;
        let real_nodes: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|p| graph.is_file_node(p))
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
            if graph.is_file_node(&edge.source)
                && graph.is_file_node(&edge.target)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::Graph;
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
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let config = Config::defaults();
        let result = PageRank.run(&make_ctx(&graph, &config));
        assert!(result.converged);
        assert_eq!(result.nodes.len(), 1);
        assert!((result.nodes[0].score - 1.0).abs() < 1e-4);
    }

    #[test]
    fn two_nodes_with_link() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let config = Config::defaults();
        let result = PageRank.run(&make_ctx(&graph, &config));
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

        let config = Config::defaults();
        let result = PageRank.run(&make_ctx(&graph, &config));
        let sum: f64 = result.nodes.iter().map(|n| n.score).sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = PageRank.run(&make_ctx(&graph, &config));
        assert!(result.converged);
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn dangling_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let config = Config::defaults();
        let result = PageRank.run(&make_ctx(&graph, &config));
        assert!(result.converged);
        let sum: f64 = result.nodes.iter().map(|n| n.score).sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }
}
