use super::{Analysis, AnalysisContext};
use crate::graph::Graph;
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, serde::Serialize)]
pub struct RedundantEdge {
    pub source: String,
    pub target: String,
    pub via: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TransitiveReductionResult {
    pub redundant_edges: Vec<RedundantEdge>,
}

pub struct TransitiveReduction;

impl Analysis for TransitiveReduction {
    type Output = TransitiveReductionResult;

    fn name(&self) -> &str {
        "transitive-reduction"
    }

    fn run(&self, ctx: &AnalysisContext) -> TransitiveReductionResult {
        let graph = ctx.graph;
        let node_set: HashSet<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();
        let mut redundant_edges = Vec::new();

        for edge in &graph.edges {
            // Skip edges to/from nodes not in the graph
            if !node_set.contains(edge.source.as_str()) || !node_set.contains(edge.target.as_str())
            {
                continue;
            }

            // Skip self-loops
            if edge.source == edge.target {
                continue;
            }

            // BFS from source, skipping the direct edge to target.
            // If target is still reachable, the edge is redundant.
            if let Some(via) =
                reachable_without_direct(graph, &edge.source, &edge.target, &node_set)
            {
                redundant_edges.push(RedundantEdge {
                    source: edge.source.clone(),
                    target: edge.target.clone(),
                    via,
                });
            }
        }

        redundant_edges.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });

        TransitiveReductionResult { redundant_edges }
    }
}

/// BFS from `source` following forward edges, but skip any direct edge to `target`.
/// If `target` is reached, return the first intermediate node on the path (the `via`).
fn reachable_without_direct(
    graph: &Graph,
    source: &str,
    target: &str,
    node_set: &HashSet<&str>,
) -> Option<String> {
    let mut visited = HashSet::new();
    // Queue entries: (current_node, first_intermediate_on_this_path)
    let mut queue: VecDeque<(&str, Option<&str>)> = VecDeque::new();

    visited.insert(source);
    queue.push_back((source, None));

    while let Some((current, first_hop)) = queue.pop_front() {
        let Some(edge_indices) = graph.forward.get(current) else {
            continue;
        };

        for &idx in edge_indices {
            let neighbor = graph.edges[idx].target.as_str();

            // Skip the direct source→target edge
            if current == source && neighbor == target {
                continue;
            }

            // Only traverse within known nodes
            if !node_set.contains(neighbor) {
                continue;
            }

            if !visited.insert(neighbor) {
                continue;
            }

            // Track the first intermediate node on this path
            let via = first_hop.unwrap_or(neighbor);

            if neighbor == target {
                return Some(via.to_string());
            }

            queue.push_back((neighbor, Some(via)));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::{Edge, Node, NodeType};
    use std::collections::HashMap;
    use std::path::Path;

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            is_graph: false,
            metadata: HashMap::new(),
            included: true,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            link: None,
            parser: "markdown".into(),
        }
    }

    fn make_ctx<'a>(graph: &'a Graph, config: &'a Config) -> AnalysisContext<'a> {
        AnalysisContext {
            graph,
            root: Path::new("."),
            config,
            lockfile: None,
        }
    }

    #[test]
    fn diamond_redundancy() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));

        assert_eq!(result.redundant_edges.len(), 1);
        assert_eq!(result.redundant_edges[0].source, "a.md");
        assert_eq!(result.redundant_edges[0].target, "c.md");
        assert_eq!(result.redundant_edges[0].via, "b.md");
    }

    #[test]
    fn no_redundancy() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));
        assert!(result.redundant_edges.is_empty());
    }

    #[test]
    fn multiple_redundancies() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("b.md", "d.md"));
        graph.add_edge(make_edge("a.md", "c.md"));
        graph.add_edge(make_edge("a.md", "d.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));

        assert_eq!(result.redundant_edges.len(), 2);
        let targets: Vec<&str> = result
            .redundant_edges
            .iter()
            .map(|r| r.target.as_str())
            .collect();
        assert!(targets.contains(&"c.md"));
        assert!(targets.contains(&"d.md"));
    }

    #[test]
    fn longer_chain() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "d.md"));
        graph.add_edge(make_edge("a.md", "d.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));

        assert_eq!(result.redundant_edges.len(), 1);
        assert_eq!(result.redundant_edges[0].source, "a.md");
        assert_eq!(result.redundant_edges[0].target, "d.md");
        assert_eq!(result.redundant_edges[0].via, "b.md");
    }

    #[test]
    fn self_loop_not_flagged() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "a.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));
        assert!(result.redundant_edges.is_empty());
    }

    #[test]
    fn disconnected_components() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("d.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("c.md", "d.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));
        assert!(result.redundant_edges.is_empty());
    }

    #[test]
    fn edge_to_missing_node_skipped() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("a.md", "missing.md"));

        let config = Config::defaults();
        let result = TransitiveReduction.run(&make_ctx(&graph, &config));
        assert!(result.redundant_edges.is_empty());
    }
}
