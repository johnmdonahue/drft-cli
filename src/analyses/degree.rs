use super::{Analysis, AnalysisContext};

#[derive(Debug, Clone, serde::Serialize)]
pub struct NodeDegree {
    pub node: String,
    pub in_degree: usize,
    pub out_degree: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DegreeResult {
    pub nodes: Vec<NodeDegree>,
}

pub struct Degree;

impl Analysis for Degree {
    type Output = DegreeResult;

    fn name(&self) -> &str {
        "degree"
    }

    fn run(&self, ctx: &AnalysisContext) -> DegreeResult {
        let graph = ctx.graph;
        let mut nodes: Vec<NodeDegree> = graph
            .nodes
            .keys()
            .filter(|path| graph.is_file_node(path))
            .map(|path| {
                let in_degree = graph
                    .reverse
                    .get(path.as_str())
                    .map(|indices| {
                        indices
                            .iter()
                            .filter(|&&idx| graph.is_file_node(&graph.edges[idx].source))
                            .count()
                    })
                    .unwrap_or(0);

                let out_degree = graph
                    .forward
                    .get(path.as_str())
                    .map(|indices| {
                        indices
                            .iter()
                            .filter(|&&idx| graph.is_file_node(&graph.edges[idx].target))
                            .count()
                    })
                    .unwrap_or(0);

                NodeDegree {
                    node: path.clone(),
                    in_degree,
                    out_degree,
                }
            })
            .collect();

        nodes.sort_by(|a, b| a.node.cmp(&b.node));

        DegreeResult { nodes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::AnalysisContext;
    use crate::config::Config;
    use crate::graph::test_helpers::{make_edge, make_node};
    use crate::graph::{Graph, Node, NodeType};
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
    fn empty_graph() {
        let graph = Graph::new();
        let config = Config::defaults();
        let result = Degree.run(&make_ctx(&graph, &config));
        assert!(result.nodes.is_empty());
    }

    #[test]
    fn single_node() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));

        let config = Config::defaults();
        let result = Degree.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].node, "a.md");
        assert_eq!(result.nodes[0].in_degree, 0);
        assert_eq!(result.nodes[0].out_degree, 0);
    }

    #[test]
    fn diamond_graph() {
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
        let result = Degree.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes.len(), 4);

        let a = result.nodes.iter().find(|n| n.node == "a.md").unwrap();
        assert_eq!(a.in_degree, 0);
        assert_eq!(a.out_degree, 2);

        let d = result.nodes.iter().find(|n| n.node == "d.md").unwrap();
        assert_eq!(d.in_degree, 2);
        assert_eq!(d.out_degree, 0);
    }

    #[test]
    fn self_loop() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_edge(make_edge("a.md", "a.md"));

        let config = Config::defaults();
        let result = Degree.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes[0].in_degree, 1);
        assert_eq!(result.nodes[0].out_degree, 1);
    }

    #[test]
    fn excludes_synthetic_nodes() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(Node {
            path: "https://example.com".into(),
            node_type: NodeType::External,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        graph.add_edge(make_edge("a.md", "https://example.com"));

        let config = Config::defaults();
        let result = Degree.run(&make_ctx(&graph, &config));
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].node, "a.md");
        assert_eq!(result.nodes[0].out_degree, 0);
    }

    #[test]
    fn sorted_by_path() {
        let mut graph = Graph::new();
        graph.add_node(make_node("c.md"));
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));

        let config = Config::defaults();
        let result = Degree.run(&make_ctx(&graph, &config));
        let paths: Vec<&str> = result.nodes.iter().map(|n| n.node.as_str()).collect();
        assert_eq!(paths, vec!["a.md", "b.md", "c.md"]);
    }
}
