use super::{Analysis, Metric, MetricKind};
use crate::graph::Graph;
use std::collections::{HashMap, VecDeque};
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeBetweenness {
    pub node: String,
    pub score: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BetweennessResult {
    pub nodes: Vec<NodeBetweenness>,
}

pub struct Betweenness;

impl Analysis for Betweenness {
    type Output = BetweennessResult;

    fn name(&self) -> &str {
        "betweenness"
    }

    fn run(&self, graph: &Graph, _root: &Path) -> BetweennessResult {
        let real_nodes: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|p| graph.is_real_node(p))
            .map(|s| s.as_str())
            .collect();

        let n = real_nodes.len();
        let mut centrality: HashMap<&str, f64> = HashMap::new();
        for &node in &real_nodes {
            centrality.insert(node, 0.0);
        }

        if n <= 2 {
            let nodes = real_nodes
                .iter()
                .map(|&node| NodeBetweenness {
                    node: node.to_string(),
                    score: 0.0,
                })
                .collect();
            let mut result = BetweennessResult { nodes };
            result.nodes.sort_by(|a, b| a.node.cmp(&b.node));
            return result;
        }

        // Brandes' algorithm (directed)
        for &s in &real_nodes {
            // BFS from s
            let mut stack: Vec<&str> = Vec::new();
            let mut predecessors: HashMap<&str, Vec<&str>> = HashMap::new();
            let mut sigma: HashMap<&str, f64> = HashMap::new();
            let mut dist: HashMap<&str, i64> = HashMap::new();

            for &node in &real_nodes {
                predecessors.insert(node, Vec::new());
                sigma.insert(node, 0.0);
                dist.insert(node, -1);
            }

            sigma.insert(s, 1.0);
            dist.insert(s, 0);

            let mut queue = VecDeque::new();
            queue.push_back(s);

            while let Some(v) = queue.pop_front() {
                stack.push(v);
                let v_dist = dist[v];

                if let Some(edge_indices) = graph.forward.get(v) {
                    for &idx in edge_indices {
                        let w = graph.edges[idx].target.as_str();
                        if !graph.is_real_node(w) {
                            continue;
                        }
                        if dist[w] < 0 {
                            dist.insert(w, v_dist + 1);
                            queue.push_back(w);
                        }
                        if dist[w] == v_dist + 1 {
                            *sigma.get_mut(w).unwrap() += sigma[v];
                            predecessors.get_mut(w).unwrap().push(v);
                        }
                    }
                }
            }

            // Back-propagation
            let mut delta: HashMap<&str, f64> = HashMap::new();
            for &node in &real_nodes {
                delta.insert(node, 0.0);
            }

            while let Some(w) = stack.pop() {
                for &v in &predecessors[w] {
                    let d = (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                    *delta.get_mut(v).unwrap() += d;
                }
                if w != s {
                    *centrality.get_mut(w).unwrap() += delta[w];
                }
            }
        }

        // Normalize by (n-1)*(n-2) for directed graphs
        let norm = ((n - 1) * (n - 2)) as f64;
        let mut nodes: Vec<NodeBetweenness> = centrality
            .into_iter()
            .map(|(node, score)| NodeBetweenness {
                node: node.to_string(),
                score: score / norm,
            })
            .collect();

        // Sort by score descending, then node ascending
        nodes.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap()
                .then_with(|| a.node.cmp(&b.node))
        });

        BetweennessResult { nodes }
    }

    fn metrics(&self, output: &BetweennessResult, _graph: &Graph) -> Vec<Metric> {
        if output.nodes.is_empty() {
            return vec![];
        }
        let max = output.nodes.iter().map(|n| n.score).fold(0.0f64, f64::max);
        let gini = gini_coefficient(&output.nodes.iter().map(|n| n.score).collect::<Vec<_>>());

        vec![
            Metric {
                name: "max_betweenness".into(),
                value: max,
                kind: MetricKind::Score,
                dimension: "consistency".into(),
            },
            Metric {
                name: "betweenness_gini".into(),
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
    fn hub_node() {
        // a → center → b, c → center → d (center is on all a→b, a→d, c→b, c→d paths)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("center.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "center.md"));
        graph.add_edge(make_edge("c.md", "center.md"));
        graph.add_edge(make_edge("center.md", "b.md"));
        graph.add_edge(make_edge("center.md", "d.md"));

        let result = Betweenness.run(&graph, Path::new("."));
        // center should have highest betweenness
        assert_eq!(result.nodes[0].node, "center.md");
        assert!(result.nodes[0].score > 0.0);
    }

    #[test]
    fn linear_chain() {
        // a → b → c → d
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let result = Betweenness.run(&graph, Path::new("."));
        // b and c are in the middle, should have highest scores
        let b = result.nodes.iter().find(|n| n.node == "b.md").unwrap();
        let c = result.nodes.iter().find(|n| n.node == "c.md").unwrap();
        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        assert!(b.score > a.score);
        assert!(c.score > a.score);
    }

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let result = Betweenness.run(&graph, Path::new("."));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].score, 0.0);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let result = Betweenness.run(&graph, Path::new("."));
        assert!(result.nodes.is_empty());
    }
}
