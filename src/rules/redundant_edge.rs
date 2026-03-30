use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
use crate::analyses::transitive_reduction::TransitiveReduction;
use crate::diagnostic::Diagnostic;
use crate::rules::{Rule, RuleContext};

pub struct RedundantEdgeRule;

impl Rule for RedundantEdgeRule {
    fn name(&self) -> &str {
        "redundant-edge"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let analysis_ctx = AnalysisContext {
            graph: ctx.graph,
            root: ctx.root,
            config: ctx.config,
            lockfile: ctx.lockfile,
        };
        let result = TransitiveReduction.run(&analysis_ctx);

        result
            .redundant_edges
            .iter()
            .map(|re| Diagnostic {
                rule: "redundant-edge".into(),
                message: "transitively redundant".into(),
                source: Some(re.source.clone()),
                target: Some(re.target.clone()),
                via: Some(re.via.clone()),
                fix: Some(format!(
                    "{} links directly to {}, but already reaches it via {} \u{2014} remove the direct link",
                    re.source, re.target, re.via
                )),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::path::Path;

    fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
        }
    }

    fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        }
    }

    fn make_ctx<'a>(graph: &'a Graph, config: &'a Config) -> RuleContext<'a> {
        RuleContext {
            graph,
            root: Path::new("."),
            config,
            lockfile: None,
        }
    }

    #[test]
    fn produces_diagnostics_for_redundant_edges() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));
        graph.add_edge(make_edge("a.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = RedundantEdgeRule.evaluate(&make_ctx(&graph, &config));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "redundant-edge");
        assert_eq!(diagnostics[0].source.as_deref(), Some("a.md"));
        assert_eq!(diagnostics[0].target.as_deref(), Some("c.md"));
        assert_eq!(diagnostics[0].via.as_deref(), Some("b.md"));
        assert_eq!(diagnostics[0].message, "transitively redundant");
    }

    #[test]
    fn no_diagnostics_when_no_redundancy() {
        let mut graph = Graph::new();
        graph.add_node(make_node("a.md"));
        graph.add_node(make_node("b.md"));
        graph.add_node(make_node("c.md"));
        graph.add_edge(make_edge("a.md", "b.md"));
        graph.add_edge(make_edge("b.md", "c.md"));

        let config = Config::defaults();
        let diagnostics = RedundantEdgeRule.evaluate(&make_ctx(&graph, &config));
        assert!(diagnostics.is_empty());
    }
}
