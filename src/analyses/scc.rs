use super::{Analysis, AnalysisContext};
use crate::graph::Graph;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Scc {
    pub id: usize,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SccResult {
    pub scc_count: usize,
    pub nontrivial_count: usize,
    pub sccs: Vec<Scc>,
    pub node_scc: HashMap<String, usize>,
}

pub struct StronglyConnectedComponents;

impl Analysis for StronglyConnectedComponents {
    type Output = SccResult;

    fn name(&self) -> &str {
        "scc"
    }

    fn run(&self, ctx: &AnalysisContext) -> SccResult {
        let graph = ctx.graph;
        let real_nodes: Vec<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();

        let mut state = TarjanState {
            graph,
            index_counter: 0,
            stack: Vec::new(),
            on_stack: HashMap::new(),
            index: HashMap::new(),
            lowlink: HashMap::new(),
            components: Vec::new(),
        };

        // Sort for deterministic output
        let mut sorted_nodes = real_nodes;
        sorted_nodes.sort();

        for &node in &sorted_nodes {
            if !state.index.contains_key(node) {
                state.strongconnect(node);
            }
        }

        // Build result
        let all_components = state.components;
        let scc_count = all_components.len();

        // Check for self-loops to determine non-trivial single-node SCCs
        let mut has_self_loop: HashMap<&str, bool> = HashMap::new();
        for edge in &graph.edges {
            if edge.source == edge.target && graph.is_internal_edge(edge) {
                has_self_loop.insert(edge.source.as_str(), true);
            }
        }

        // Non-trivial SCCs: size > 1, or size == 1 with a self-loop
        let nontrivial: Vec<Vec<String>> = all_components
            .iter()
            .filter(|c| c.len() > 1 || (c.len() == 1 && has_self_loop.contains_key(c[0].as_str())))
            .cloned()
            .collect();

        let nontrivial_count = nontrivial.len();

        // Assign SCC IDs to all nodes
        let mut node_scc: HashMap<String, usize> = HashMap::new();
        for (i, component) in all_components.iter().enumerate() {
            for member in component {
                node_scc.insert(member.clone(), i + 1);
            }
        }

        // Build output SCCs (non-trivial only, sorted by size desc then first member)
        let mut output_sccs = nontrivial;
        output_sccs.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));

        let sccs = output_sccs
            .into_iter()
            .enumerate()
            .map(|(i, members)| Scc { id: i + 1, members })
            .collect();

        SccResult {
            scc_count,
            nontrivial_count,
            sccs,
            node_scc,
        }
    }
}

struct TarjanState<'a> {
    graph: &'a Graph,
    index_counter: usize,
    stack: Vec<&'a str>,
    on_stack: HashMap<&'a str, bool>,
    index: HashMap<&'a str, usize>,
    lowlink: HashMap<&'a str, usize>,
    components: Vec<Vec<String>>,
}

impl<'a> TarjanState<'a> {
    fn strongconnect(&mut self, v: &'a str) {
        self.index.insert(v, self.index_counter);
        self.lowlink.insert(v, self.index_counter);
        self.index_counter += 1;
        self.stack.push(v);
        self.on_stack.insert(v, true);

        // Visit successors
        if let Some(edge_indices) = self.graph.forward.get(v) {
            for &idx in edge_indices {
                let edge = &self.graph.edges[idx];
                if !self.graph.is_internal_edge(edge) {
                    continue;
                }
                let w = edge.target.as_str();
                if !self.index.contains_key(w) {
                    self.strongconnect(w);
                    let w_low = self.lowlink[w];
                    let v_low = self.lowlink[v];
                    if w_low < v_low {
                        self.lowlink.insert(v, w_low);
                    }
                } else if self.on_stack.get(w) == Some(&true) {
                    let w_idx = self.index[w];
                    let v_low = self.lowlink[v];
                    if w_idx < v_low {
                        self.lowlink.insert(v, w_idx);
                    }
                }
            }
        }

        // If v is a root node, pop the stack to form an SCC
        if self.lowlink[v] == self.index[v] {
            let mut component = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack.insert(w, false);
                component.push(w.to_string());
                if w == v {
                    break;
                }
            }
            component.sort();
            self.components.push(component);
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
    fn dag_has_no_nontrivial_sccs() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.scc_count, 3);
        assert_eq!(result.nontrivial_count, 0);
        assert!(result.sccs.is_empty());
    }

    #[test]
    fn simple_cycle() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.nontrivial_count, 1);
        assert_eq!(result.sccs.len(), 1);
        assert_eq!(result.sccs[0].members, vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn two_separate_cycles() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "a.md"));
        graph.add_edge(make_edge("c.md", "d.md"));
        graph.add_edge(make_edge("d.md", "c.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.nontrivial_count, 2);
        assert_eq!(result.sccs.len(), 2);
    }

    #[test]
    fn mixed_cyclic_and_acyclic() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "b.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.nontrivial_count, 1);
        assert_eq!(result.sccs[0].members, vec!["b.md", "c.md"]);
        assert!(result.node_scc.contains_key("a.md"));
    }

    #[test]
    fn self_loop_is_nontrivial() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "a.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.nontrivial_count, 1);
        assert_eq!(result.sccs[0].members, vec!["a.md"]);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.scc_count, 0);
        assert_eq!(result.nontrivial_count, 0);
    }

    #[test]
    fn every_node_has_scc_id() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = StronglyConnectedComponents.run(&make_ctx(&graph, &config));
        assert!(result.node_scc.contains_key("a.md"));
        assert!(result.node_scc.contains_key("b.md"));
        assert!(result.node_scc.contains_key("c.md"));
    }
}
