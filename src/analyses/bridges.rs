use super::{Analysis, AnalysisContext};
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct BridgeEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BridgesResult {
    pub cut_vertices: Vec<String>,
    pub bridges: Vec<BridgeEdge>,
}

pub struct Bridges;

impl Analysis for Bridges {
    type Output = BridgesResult;

    fn name(&self) -> &str {
        "bridges"
    }

    fn run(&self, ctx: &AnalysisContext) -> BridgesResult {
        let graph = ctx.graph;
        // Build undirected adjacency among real nodes
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        let real_nodes: Vec<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();

        for node in &real_nodes {
            adj.entry(node).or_default();
        }

        for edge in &graph.edges {
            if graph.is_internal_edge(edge) && edge.source != edge.target {
                adj.entry(edge.source.as_str())
                    .or_default()
                    .push(edge.target.as_str());
                adj.entry(edge.target.as_str())
                    .or_default()
                    .push(edge.source.as_str());
            }
        }

        // Deduplicate adjacency lists
        for neighbors in adj.values_mut() {
            neighbors.sort();
            neighbors.dedup();
        }

        let mut state = TarjanBridgeState {
            adj: &adj,
            timer: 0,
            disc: HashMap::new(),
            low: HashMap::new(),
            parent: HashMap::new(),
            cut_vertices: Vec::new(),
            bridges: Vec::new(),
        };

        // Sort for deterministic traversal
        let mut sorted_nodes = real_nodes;
        sorted_nodes.sort();

        for &node in &sorted_nodes {
            if !state.disc.contains_key(node) {
                state.dfs(node);
            }
        }

        let mut cut_vertices = state.cut_vertices;
        cut_vertices.sort();
        cut_vertices.dedup();

        let mut bridges: Vec<BridgeEdge> = state
            .bridges
            .into_iter()
            .map(|(a, b)| {
                // Normalize order
                if a < b {
                    BridgeEdge {
                        source: a.to_string(),
                        target: b.to_string(),
                    }
                } else {
                    BridgeEdge {
                        source: b.to_string(),
                        target: a.to_string(),
                    }
                }
            })
            .collect();
        bridges.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });
        bridges.dedup_by(|a, b| a.source == b.source && a.target == b.target);

        BridgesResult {
            cut_vertices: cut_vertices.into_iter().map(|s| s.to_string()).collect(),
            bridges,
        }
    }
}

struct TarjanBridgeState<'a> {
    adj: &'a HashMap<&'a str, Vec<&'a str>>,
    timer: usize,
    disc: HashMap<&'a str, usize>,
    low: HashMap<&'a str, usize>,
    parent: HashMap<&'a str, &'a str>,
    cut_vertices: Vec<&'a str>,
    bridges: Vec<(&'a str, &'a str)>,
}

impl<'a> TarjanBridgeState<'a> {
    fn dfs(&mut self, u: &'a str) {
        self.disc.insert(u, self.timer);
        self.low.insert(u, self.timer);
        self.timer += 1;
        let mut child_count = 0;

        let neighbors = self.adj.get(u).cloned().unwrap_or_default();
        for v in neighbors {
            if !self.disc.contains_key(v) {
                child_count += 1;
                self.parent.insert(v, u);
                self.dfs(v);

                let v_low = self.low[v];
                let u_low = self.low[u];
                if v_low < u_low {
                    self.low.insert(u, v_low);
                }

                // u is a cut vertex if:
                // 1) u is root of DFS tree and has two or more children
                let is_root = !self.parent.contains_key(u);
                if is_root && child_count > 1 {
                    self.cut_vertices.push(u);
                }
                // 2) u is not root and low[v] >= disc[u]
                if !is_root && self.low[v] >= self.disc[u] {
                    self.cut_vertices.push(u);
                }

                // (u, v) is a bridge if low[v] > disc[u]
                if self.low[v] > self.disc[u] {
                    self.bridges.push((u, v));
                }
            } else if self.parent.get(u) != Some(&v) {
                // Back edge
                let v_disc = self.disc[v];
                let u_low = self.low[u];
                if v_disc < u_low {
                    self.low.insert(u, v_disc);
                }
            }
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
    fn linear_chain() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert_eq!(result.bridges.len(), 2);
        assert_eq!(result.cut_vertices, vec!["b.md"]);
    }

    #[test]
    fn cycle_has_no_bridges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert!(result.bridges.is_empty());
        assert!(result.cut_vertices.is_empty());
    }

    #[test]
    fn star_graph() {
        let mut graph = Graph::new();
        graph.add_node(make_node("center.md"));
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("center.md", "a.md"));
        graph.add_edge(make_edge("center.md", "b.md"));
        graph.add_edge(make_edge("center.md", "c.md"));

        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert_eq!(result.cut_vertices, vec!["center.md"]);
        assert_eq!(result.bridges.len(), 3);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert!(result.cut_vertices.is_empty());
        assert!(result.bridges.is_empty());
    }

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert!(result.cut_vertices.is_empty());
        assert!(result.bridges.is_empty());
    }

    #[test]
    fn cycle_with_tail() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let config = Config::defaults();
        let result = Bridges.run(&make_ctx(&graph, &config));
        assert_eq!(result.bridges.len(), 1);
        assert_eq!(result.bridges[0].source, "c.md");
        assert_eq!(result.bridges[0].target, "d.md");
        assert!(result.cut_vertices.contains(&"c.md".to_string()));
    }
}
