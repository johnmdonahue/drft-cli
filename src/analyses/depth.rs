use super::{Analysis, AnalysisContext};
use crate::analyses::scc::StronglyConnectedComponents;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeDepth {
    pub node: String,
    pub depth: usize,
    pub in_cycle: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DepthResult {
    pub max_depth: usize,
    pub nodes: Vec<NodeDepth>,
}

pub struct Depth;

impl Analysis for Depth {
    type Output = DepthResult;

    fn name(&self) -> &str {
        "depth"
    }

    fn run(&self, ctx: &AnalysisContext) -> DepthResult {
        let graph = ctx.graph;
        // Step 1: Run SCC analysis to identify cycles
        let scc_result = StronglyConnectedComponents.run(ctx);

        // Identify which nodes are in non-trivial SCCs
        let nontrivial_nodes: HashSet<&str> = scc_result
            .sccs
            .iter()
            .flat_map(|s| s.members.iter().map(|m| m.as_str()))
            .collect();

        // Step 2: Build condensation DAG
        // Map each real node to its super-node ID (SCC ID)
        // For non-trivial SCCs, all members map to the same super-node
        let node_to_super: &HashMap<String, usize> = &scc_result.node_scc;

        // Collect unique super-node IDs
        let super_nodes: HashSet<usize> = node_to_super.values().copied().collect();

        // Build forward adjacency for the condensation DAG
        let mut condensed_forward: HashMap<usize, HashSet<usize>> = HashMap::new();
        let mut condensed_reverse: HashMap<usize, HashSet<usize>> = HashMap::new();
        for super_id in &super_nodes {
            condensed_forward.entry(*super_id).or_default();
            condensed_reverse.entry(*super_id).or_default();
        }

        for edge in &graph.edges {
            if !graph.is_internal_edge(edge) {
                continue;
            }
            let src_super = node_to_super[&edge.source];
            let tgt_super = node_to_super[&edge.target];
            if src_super != tgt_super {
                condensed_forward
                    .entry(src_super)
                    .or_default()
                    .insert(tgt_super);
                condensed_reverse
                    .entry(tgt_super)
                    .or_default()
                    .insert(src_super);
            }
        }

        // Step 3: BFS from roots (super-nodes with no incoming edges in condensation)
        let roots: Vec<usize> = super_nodes
            .iter()
            .filter(|id| condensed_reverse.get(id).is_some_and(|s| s.is_empty()))
            .copied()
            .collect();

        let mut super_depth: HashMap<usize, usize> = HashMap::new();
        let mut queue = VecDeque::new();

        for root_id in &roots {
            super_depth.insert(*root_id, 0);
            queue.push_back(*root_id);
        }

        while let Some(current) = queue.pop_front() {
            let current_depth = super_depth[&current];
            if let Some(neighbors) = condensed_forward.get(&current) {
                for &neighbor in neighbors {
                    let new_depth = current_depth + 1;
                    let entry = super_depth.entry(neighbor).or_insert(0);
                    // Use max depth (longest path from any root)
                    if new_depth > *entry {
                        *entry = new_depth;
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        // Step 4: Expand back to per-node depths
        let mut nodes: Vec<NodeDepth> = Vec::new();
        let mut max_depth: usize = 0;

        let mut real_nodes: Vec<&str> = graph.included_nodes().map(|(s, _)| s.as_str()).collect();
        real_nodes.sort();

        for node in real_nodes {
            let super_id = node_to_super[node];
            let depth = super_depth.get(&super_id).copied().unwrap_or(0);
            let in_cycle = nontrivial_nodes.contains(node);
            max_depth = max_depth.max(depth);
            nodes.push(NodeDepth {
                node: node.to_string(),
                depth,
                in_cycle,
            });
        }

        // Sort by depth ascending, then path alphabetically
        nodes.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.node.cmp(&b.node)));

        DepthResult { max_depth, nodes }
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
        let result = Depth.run(&make_ctx(&graph, &config));
        assert_eq!(result.max_depth, 2);

        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        assert_eq!(a.depth, 0);
        assert!(!a.in_cycle);

        let c = result.nodes.iter().find(|n| n.node == "c.md").unwrap();
        assert_eq!(c.depth, 2);
    }

    #[test]
    fn diamond() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));
        graph.add_edge(make_edge("b.md", "d.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let config = Config::defaults();
        let result = Depth.run(&make_ctx(&graph, &config));
        let d = result.nodes.iter().find(|n| n.node == "d.md").unwrap();
        assert_eq!(d.depth, 2);
    }

    #[test]
    fn cycle_gets_depth_and_flag() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "b.md"));

        let config = Config::defaults();
        let result = Depth.run(&make_ctx(&graph, &config));

        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        assert_eq!(a.depth, 0);
        assert!(!a.in_cycle);

        let b = result.nodes.iter().find(|n| n.node == "b.md").unwrap();
        assert!(b.in_cycle);
        assert_eq!(b.depth, 1);

        let c = result.nodes.iter().find(|n| n.node == "c.md").unwrap();
        assert!(c.in_cycle);
        assert_eq!(c.depth, 1);
    }

    #[test]
    fn entirely_cyclic() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = Depth.run(&make_ctx(&graph, &config));
        for nd in &result.nodes {
            assert_eq!(nd.depth, 0);
            assert!(nd.in_cycle);
        }
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = Depth.run(&make_ctx(&graph, &config));
        assert_eq!(result.max_depth, 0);
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn multiple_roots() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "c.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = Depth.run(&make_ctx(&graph, &config));
        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        let b = result.nodes.iter().find(|n| n.node == "b.md").unwrap();
        let c = result.nodes.iter().find(|n| n.node == "c.md").unwrap();
        assert_eq!(a.depth, 0);
        assert_eq!(b.depth, 0);
        assert_eq!(c.depth, 1);
    }
}
