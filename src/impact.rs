//! `drft impact`: list the nodes transitively affected by a change to the seed
//! nodes, as a flat, instruction-bearing list sorted by review priority. The
//! ranking metrics (`impact_radius`, `betweenness`) are computed inline over the
//! composed graph — there is no separate analyses layer.
//!
//! Traversal is cycle-safe (a visited set), so a dependency cycle resolves
//! without looping.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::model::Graph;

/// Adjacency: a path mapped to the paths it connects to.
type Adjacency<'a> = HashMap<&'a str, Vec<&'a str>>;

/// Which way to walk from the seed. Inbound (the default) finds dependents —
/// what must be reviewed when the seed changes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

/// One impacted node with its causal chain and ranking metrics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Impacted {
    pub node: String,
    /// The neighbor through which this node was reached.
    pub via: String,
    pub depth: usize,
    /// Count of nodes that transitively depend on this node.
    pub impact_radius: usize,
    /// Betweenness centrality of this node in the whole graph.
    pub betweenness: f64,
    pub fix: String,
}

/// Compute the impacted set for `seeds`. Seeds must be node paths present in
/// `graph`; unknown seeds are skipped.
pub fn compute(
    graph: &Graph,
    seeds: &[String],
    direction: Direction,
    max_depth: Option<usize>,
) -> Vec<Impacted> {
    let (forward, reverse) = adjacency(graph);
    let betweenness = betweenness(graph);

    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();

    for seed in seeds {
        if let Some((key, _)) = graph.nodes.get_key_value(seed) {
            let seed = key.as_str();
            if visited.insert(seed) {
                queue.push_back((seed, 0));
            }
        }
    }

    // (node, via, depth)
    let mut reached: Vec<(&str, &str, usize)> = Vec::new();

    while let Some((node, depth)) = queue.pop_front() {
        if max_depth.is_some_and(|max| depth >= max) {
            continue;
        }
        let mut neighbors: Vec<&str> = Vec::new();
        if matches!(direction, Direction::Inbound | Direction::Both) {
            neighbors.extend(reverse.get(node).into_iter().flatten().copied());
        }
        if matches!(direction, Direction::Outbound | Direction::Both) {
            neighbors.extend(forward.get(node).into_iter().flatten().copied());
        }
        for next in neighbors {
            if visited.insert(next) {
                reached.push((next, node, depth + 1));
                queue.push_back((next, depth + 1));
            }
        }
    }

    let mut impacted: Vec<Impacted> = reached
        .into_iter()
        .map(|(node, via, depth)| Impacted {
            node: node.to_string(),
            via: via.to_string(),
            depth,
            impact_radius: reverse_reach_count(&reverse, node),
            betweenness: betweenness.get(node).copied().unwrap_or(0.0),
            fix: format!(
                "{via} may change — review {node} to ensure it still accurately reflects {via}"
            ),
        })
        .collect();

    // Review priority: high impact_radius at shallow depth first, then betweenness.
    impacted.sort_by(|a, b| {
        let pa = a.impact_radius as f64 / a.depth as f64 + a.betweenness;
        let pb = b.impact_radius as f64 / b.depth as f64 + b.betweenness;
        pb.partial_cmp(&pa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.node.cmp(&b.node))
    });
    impacted
}

/// Build forward (source → targets) and reverse (target → sources) adjacency
/// from the composed graph's edges.
fn adjacency(graph: &Graph) -> (Adjacency<'_>, Adjacency<'_>) {
    let mut forward: Adjacency = HashMap::new();
    let mut reverse: Adjacency = HashMap::new();
    for edge in &graph.edges {
        forward
            .entry(edge.source.as_str())
            .or_default()
            .push(edge.target.as_str());
        reverse
            .entry(edge.target.as_str())
            .or_default()
            .push(edge.source.as_str());
    }
    (forward, reverse)
}

/// Count the nodes transitively reachable from `node` along reverse edges — its
/// transitive dependents.
fn reverse_reach_count(reverse: &Adjacency, node: &str) -> usize {
    let mut visited: HashSet<&str> = HashSet::new();
    visited.insert(node);
    let mut queue: VecDeque<&str> = VecDeque::new();
    queue.push_back(node);
    let mut count = 0;
    while let Some(current) = queue.pop_front() {
        for &dependent in reverse.get(current).into_iter().flatten() {
            if visited.insert(dependent) {
                count += 1;
                queue.push_back(dependent);
            }
        }
    }
    count
}

/// Betweenness centrality (Brandes' algorithm, directed) over the resolved node
/// set and internal edges (both endpoints are graph nodes).
fn betweenness(graph: &Graph) -> HashMap<&str, f64> {
    let nodes: Vec<&str> = graph.nodes.keys().map(String::as_str).collect();
    let mut centrality: HashMap<&str, f64> = nodes.iter().map(|&n| (n, 0.0)).collect();

    let n = nodes.len();
    if n <= 2 {
        return centrality;
    }

    // Internal forward adjacency: edges whose target is a graph node.
    let node_set: HashSet<&str> = nodes.iter().copied().collect();
    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &graph.edges {
        if node_set.contains(edge.target.as_str()) {
            forward
                .entry(edge.source.as_str())
                .or_default()
                .push(edge.target.as_str());
        }
    }

    for &s in &nodes {
        let mut stack: Vec<&str> = Vec::new();
        let mut predecessors: HashMap<&str, Vec<&str>> =
            nodes.iter().map(|&n| (n, Vec::new())).collect();
        let mut sigma: HashMap<&str, f64> = nodes.iter().map(|&n| (n, 0.0)).collect();
        let mut dist: HashMap<&str, i64> = nodes.iter().map(|&n| (n, -1)).collect();

        sigma.insert(s, 1.0);
        dist.insert(s, 0);

        let mut queue = VecDeque::new();
        queue.push_back(s);

        while let Some(v) = queue.pop_front() {
            stack.push(v);
            let v_dist = dist[v];
            for &w in forward.get(v).into_iter().flatten() {
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

        let mut delta: HashMap<&str, f64> = nodes.iter().map(|&n| (n, 0.0)).collect();
        while let Some(w) = stack.pop() {
            // Each w is popped once, so its predecessor list is read once — take
            // it instead of cloning.
            let preds = std::mem::take(predecessors.get_mut(w).unwrap());
            for v in preds {
                let contribution = (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                *delta.get_mut(v).unwrap() += contribution;
            }
            if w != s {
                *centrality.get_mut(w).unwrap() += delta[w];
            }
        }
    }

    let norm = ((n - 1) * (n - 2)) as f64;
    for score in centrality.values_mut() {
        *score /= norm;
    }
    centrality
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::compose;
    use crate::model::{Edge, GraphSet, Node};
    use serde_json::json;

    fn fs_node() -> Node {
        Node::new(
            json!({ "type": "file", "hash": "b3:x" })
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    /// Build a composed graph from (source, target) edges; every referenced path
    /// becomes an fs node.
    fn graph_from(edges: &[(&str, &str)]) -> Graph {
        let mut fs = Graph::labeled("fs");
        for (s, t) in edges {
            fs.set_node(*s, fs_node());
            fs.set_node(*t, fs_node());
            fs.add_edge(Edge::new(*s, *t));
        }
        compose(&GraphSet::new(vec![fs]))
    }

    fn impacted_nodes(impacted: &[Impacted]) -> Vec<&str> {
        impacted.iter().map(|i| i.node.as_str()).collect()
    }

    #[test]
    fn inbound_finds_transitive_dependents() {
        // a -> b -> c: impact of c is b (depth 1) and a (depth 2).
        let graph = graph_from(&[("a.md", "b.md"), ("b.md", "c.md")]);
        let impacted = compute(&graph, &["c.md".into()], Direction::Inbound, None);
        let nodes = impacted_nodes(&impacted);
        assert!(nodes.contains(&"b.md"));
        assert!(nodes.contains(&"a.md"));
        let b = impacted.iter().find(|i| i.node == "b.md").unwrap();
        assert_eq!(b.depth, 1);
        assert_eq!(b.via, "c.md");
        let a = impacted.iter().find(|i| i.node == "a.md").unwrap();
        assert_eq!(a.depth, 2);
    }

    #[test]
    fn cycle_terminates() {
        // a -> b -> c -> a: impact of a is b and c, no infinite loop.
        let graph = graph_from(&[("a.md", "b.md"), ("b.md", "c.md"), ("c.md", "a.md")]);
        let impacted = compute(&graph, &["a.md".into()], Direction::Inbound, None);
        let nodes = impacted_nodes(&impacted);
        assert_eq!(nodes.len(), 2, "seed excluded, others reached once");
        assert!(nodes.contains(&"b.md") && nodes.contains(&"c.md"));
    }

    #[test]
    fn depth_limit_truncates() {
        let graph = graph_from(&[("a.md", "b.md"), ("b.md", "c.md")]);
        let impacted = compute(&graph, &["c.md".into()], Direction::Inbound, Some(1));
        let nodes = impacted_nodes(&impacted);
        assert_eq!(nodes, vec!["b.md"], "depth 1 stops before a.md");
    }

    #[test]
    fn outbound_finds_dependencies() {
        let graph = graph_from(&[("a.md", "b.md"), ("b.md", "c.md")]);
        let impacted = compute(&graph, &["a.md".into()], Direction::Outbound, None);
        let nodes = impacted_nodes(&impacted);
        assert!(nodes.contains(&"b.md") && nodes.contains(&"c.md"));
    }

    #[test]
    fn radius_counts_transitive_dependents() {
        let graph = graph_from(&[("a.md", "b.md"), ("b.md", "c.md")]);
        let impacted = compute(&graph, &["c.md".into()], Direction::Inbound, None);
        // b.md has one transitive dependent (a.md).
        let b = impacted.iter().find(|i| i.node == "b.md").unwrap();
        assert_eq!(b.impact_radius, 1);
    }
}
