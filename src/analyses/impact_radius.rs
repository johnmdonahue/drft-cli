use super::{Analysis, AnalysisContext};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImpactRadiusNode {
    pub node: String,
    /// Count of transitive dependents (nodes reachable via reverse edges).
    pub radius: usize,
    /// Count of direct dependents (reverse neighbors).
    pub direct_dependents: usize,
    /// Longest reverse path from this node to a dependent.
    pub max_depth: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImpactRadiusResult {
    pub nodes: Vec<ImpactRadiusNode>,
}

pub struct ImpactRadius;

impl Analysis for ImpactRadius {
    type Output = ImpactRadiusResult;

    fn name(&self) -> &str {
        "impact-radius"
    }

    fn run(&self, ctx: &AnalysisContext) -> ImpactRadiusResult {
        let graph = ctx.graph;

        let mut nodes: Vec<ImpactRadiusNode> = graph
            .nodes
            .keys()
            .map(|path| {
                let mut visited = HashSet::new();
                let mut queue = VecDeque::new();
                visited.insert(path.as_str());
                queue.push_back((path.as_str(), 0usize));

                let mut direct_dependents = 0usize;
                let mut max_depth = 0usize;
                let mut radius = 0usize;

                while let Some((current, depth)) = queue.pop_front() {
                    if let Some(edge_indices) = graph.reverse.get(current) {
                        for &idx in edge_indices {
                            let dependent = graph.edges[idx].source.as_str();
                            if visited.insert(dependent) {
                                let next_depth = depth + 1;
                                radius += 1;
                                if next_depth == 1 {
                                    direct_dependents += 1;
                                }
                                if next_depth > max_depth {
                                    max_depth = next_depth;
                                }
                                queue.push_back((dependent, next_depth));
                            }
                        }
                    }
                }

                ImpactRadiusNode {
                    node: path.clone(),
                    radius,
                    direct_dependents,
                    max_depth,
                }
            })
            .collect();

        nodes.sort_by(|a, b| a.node.cmp(&b.node));

        ImpactRadiusResult { nodes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Edge, Graph, Location, TargetKind};
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
        let result = ImpactRadius.run(&make_ctx(&graph, &config));
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn single_node_no_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let config = Config::defaults();
        let result = ImpactRadius.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].radius, 0);
        assert_eq!(result.nodes[0].direct_dependents, 0);
        assert_eq!(result.nodes[0].max_depth, 0);
    }

    #[test]
    fn linear_chain() {
        // a -> b -> c -> d
        // d has radius 3 (a, b, c depend on it transitively)
        // c has radius 2 (a, b)
        // b has radius 1 (a)
        // a has radius 0
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let config = Config::defaults();
        let result = ImpactRadius.run(&make_ctx(&graph, &config));

        let get = |name: &str| result.nodes.iter().find(|n| n.node == name).unwrap();

        assert_eq!(get("d.md").radius, 3);
        assert_eq!(get("d.md").direct_dependents, 1);
        assert_eq!(get("d.md").max_depth, 3);

        assert_eq!(get("c.md").radius, 2);
        assert_eq!(get("c.md").direct_dependents, 1);
        assert_eq!(get("c.md").max_depth, 2);

        assert_eq!(get("b.md").radius, 1);
        assert_eq!(get("b.md").direct_dependents, 1);
        assert_eq!(get("b.md").max_depth, 1);

        assert_eq!(get("a.md").radius, 0);
        assert_eq!(get("a.md").direct_dependents, 0);
        assert_eq!(get("a.md").max_depth, 0);
    }

    #[test]
    fn diamond_graph() {
        // a -> b, a -> c, b -> d, c -> d
        // d has radius 3 (b, c, a)
        // b has radius 1 (a), c has radius 1 (a)
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
        let result = ImpactRadius.run(&make_ctx(&graph, &config));

        let get = |name: &str| result.nodes.iter().find(|n| n.node == name).unwrap();

        assert_eq!(get("d.md").radius, 3);
        assert_eq!(get("d.md").direct_dependents, 2);
        assert_eq!(get("d.md").max_depth, 2);

        assert_eq!(get("b.md").radius, 1);
        assert_eq!(get("b.md").direct_dependents, 1);

        assert_eq!(get("a.md").radius, 0);
    }

    #[test]
    fn cycle() {
        // a -> b -> c -> a (cycle)
        // Each node has radius 2 (can reach the other two via reverse edges)
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let config = Config::defaults();
        let result = ImpactRadius.run(&make_ctx(&graph, &config));

        let get = |name: &str| result.nodes.iter().find(|n| n.node == name).unwrap();

        assert_eq!(get("a.md").radius, 2);
        assert_eq!(get("b.md").radius, 2);
        assert_eq!(get("c.md").radius, 2);
    }

    #[test]
    fn external_edges_dont_create_nodes() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(Edge {
            source: "a.md".into(),
            target: "https://example.com".into(),
            target_kind: TargetKind::External(Location::Remote),
            link: None,
            parser: "markdown".into(),
        });

        let config = Config::defaults();
        let result = ImpactRadius.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].radius, 0);
    }
}
