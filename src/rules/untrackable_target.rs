use crate::diagnostic::Diagnostic;
use crate::graph::NodeType;
use crate::rules::{Rule, RuleContext};

pub struct UntrackableTargetRule;

impl Rule for UntrackableTargetRule {
    fn name(&self) -> &str {
        "untrackable-target"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let graph = &ctx.graph.graph;

        graph
            .edges
            .iter()
            .filter_map(|edge| {
                let node = graph.nodes.get(&edge.target)?;
                if node.node_type != NodeType::Directory || node.hash.is_some() {
                    return None;
                }

                Some(Diagnostic {
                    rule: "untrackable-target".into(),
                    message: "directory has no drft.toml — cannot track for staleness".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "add a drft.toml to {t} to declare it as a graph",
                        t = edge.target
                    )),
                    ..Default::default()
                })
            })
            .collect()
    }
}
