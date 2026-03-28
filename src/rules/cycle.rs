use std::collections::{HashMap, HashSet};

use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct CycleRule;

#[derive(Clone, Copy, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

impl Rule for CycleRule {
    fn name(&self) -> &str {
        "cycle"
    }

    fn evaluate(&self, graph: &Graph, _root: &Path) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let mut color: HashMap<&str, Color> = HashMap::new();
        let mut stack: Vec<&str> = Vec::new();

        for node_id in graph.nodes.keys() {
            color.insert(node_id.as_str(), Color::White);
        }

        let node_set: HashSet<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();

        for node_id in graph.nodes.keys() {
            if color[node_id.as_str()] == Color::White {
                if let Some(cycle_path) = dfs(node_id, graph, &node_set, &mut color, &mut stack) {
                    let fix = format!(
                        "circular dependency — review whether one of these links can be removed or the content restructured: {}",
                        cycle_path.join(" → ")
                    );
                    diagnostics.push(Diagnostic {
                        rule: "cycle".into(),
                        message: "cycle detected".into(),
                        path: Some(cycle_path),
                        fix: Some(fix),
                        ..Default::default()
                    });
                }
            }
        }

        diagnostics
    }
}

fn dfs<'a>(
    node: &'a str,
    graph: &'a Graph,
    node_set: &HashSet<&str>,
    color: &mut HashMap<&'a str, Color>,
    stack: &mut Vec<&'a str>,
) -> Option<Vec<String>> {
    color.insert(node, Color::Gray);
    stack.push(node);

    if let Some(edge_indices) = graph.forward.get(node) {
        for &idx in edge_indices {
            let target = graph.edges[idx].target.as_str();

            if !node_set.contains(target) {
                continue;
            }

            match color.get(target) {
                Some(Color::White) => {
                    if let Some(cycle) = dfs(target, graph, node_set, color, stack) {
                        return Some(cycle);
                    }
                }
                Some(Color::Gray) => {
                    // Found a back edge — extract cycle from the stack
                    let start = stack.iter().position(|&n| n == target).unwrap();
                    let mut cycle: Vec<String> =
                        stack[start..].iter().map(|s| s.to_string()).collect();
                    cycle.push(target.to_string()); // close the cycle
                    return Some(cycle);
                }
                _ => {}
            }
        }
    }

    stack.pop();
    color.insert(node, Color::Black);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::Document,
            hash: None,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            edge_type: EdgeType::Inline,
        }
    }

    #[test]
    fn detects_simple_cycle() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("c.md", "a.md"));

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "cycle");

        let path = diagnostics[0].path.as_ref().unwrap();
        // Cycle should start and end with the same node
        assert_eq!(path.first(), path.last());
        assert_eq!(path.len(), 4); // a → b → c → a
    }

    #[test]
    fn no_cycle_in_dag() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn ignores_broken_link_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        // Edge to non-existent node should not crash or produce false cycle
        graph.add_edge(make_edge("a.md", "missing.md"));

        let rule = CycleRule;
        let diagnostics = rule.evaluate(&graph, Path::new("."));
        assert!(diagnostics.is_empty());
    }
}
