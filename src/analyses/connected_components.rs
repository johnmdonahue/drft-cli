use super::{Analysis, AnalysisContext};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Component {
    pub id: usize,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ConnectedComponentsResult {
    pub component_count: usize,
    pub components: Vec<Component>,
}

pub struct ConnectedComponents;

impl Analysis for ConnectedComponents {
    type Output = ConnectedComponentsResult;

    fn name(&self) -> &str {
        "connected-components"
    }

    fn run(&self, ctx: &AnalysisContext) -> ConnectedComponentsResult {
        let graph = ctx.graph;
        // Build undirected adjacency among real nodes
        let real_nodes: Vec<&str> = graph
            .nodes
            .keys()
            .filter(|p| graph.is_file_node(p))
            .map(|s| s.as_str())
            .collect();

        let mut adj: HashMap<&str, HashSet<&str>> = HashMap::new();
        for node in &real_nodes {
            adj.entry(node).or_default();
        }

        for edge in &graph.edges {
            if graph.is_file_node(&edge.source) && graph.is_file_node(&edge.target) {
                adj.entry(edge.source.as_str())
                    .or_default()
                    .insert(edge.target.as_str());
                adj.entry(edge.target.as_str())
                    .or_default()
                    .insert(edge.source.as_str());
            }
        }

        // BFS to find components
        let mut visited: HashSet<&str> = HashSet::new();
        let mut components = Vec::new();

        // Sort real_nodes for deterministic iteration order
        let mut sorted_nodes = real_nodes;
        sorted_nodes.sort();

        for &node in &sorted_nodes {
            if visited.contains(node) {
                continue;
            }

            let mut members = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(node);
            visited.insert(node);

            while let Some(current) = queue.pop_front() {
                members.push(current.to_string());
                if let Some(neighbors) = adj.get(current) {
                    let mut sorted_neighbors: Vec<&str> = neighbors.iter().copied().collect();
                    sorted_neighbors.sort();
                    for neighbor in sorted_neighbors {
                        if visited.insert(neighbor) {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }

            members.sort();
            components.push(members);
        }

        // Sort components by size descending, then by first member
        components.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a[0].cmp(&b[0])));

        let component_count = components.len();
        let components = components
            .into_iter()
            .enumerate()
            .map(|(i, members)| Component { id: i + 1, members })
            .collect();

        ConnectedComponentsResult {
            component_count,
            components,
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
    use crate::graph::{Node, NodeType};
    use std::collections::HashMap;
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
    fn single_component() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 1);
        assert_eq!(result.components[0].members.len(), 3);
    }

    #[test]
    fn two_components() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 2);
        assert_eq!(result.components[0].members.len(), 2);
        assert_eq!(result.components[1].members.len(), 2);
    }

    #[test]
    fn isolated_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 2);
        assert_eq!(result.components[0].members, vec!["a.md", "b.md"]);
        assert_eq!(result.components[1].members, vec!["c.md"]);
    }

    #[test]
    fn excludes_external_nodes() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(Node {
            path: "https://example.com".into(),
            node_type: NodeType::External,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
        });
        graph.add_edge(make_edge("a.md", "https://example.com"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 1);
        assert_eq!(result.components[0].members, vec!["a.md"]);
    }

    #[test]
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 0);
        assert!(result.components.is_empty());
    }

    #[test]
    fn undirected_connectivity() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("c.md", "b.md"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 1);
        assert_eq!(result.components[0].members.len(), 3);
    }

    #[test]
    fn sorted_by_size_descending() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_node(make_node("e.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let config = Config::defaults();
        let result = ConnectedComponents.run(&make_ctx(&graph, &config));
        assert_eq!(result.component_count, 3);
        assert_eq!(result.components[0].members.len(), 3);
        assert_eq!(result.components[1].members.len(), 1);
        assert_eq!(result.components[2].members.len(), 1);
    }
}
