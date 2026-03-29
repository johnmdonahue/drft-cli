use super::{AnalysisResults, Metric, MetricKind};
use crate::analyses::edge_classification::EdgeStatus;

pub fn extract(results: &AnalysisResults) -> Vec<Metric> {
    let mut metrics = Vec::new();

    // From degree analysis
    let total_nodes = results.degree.nodes.len() as f64;
    if total_nodes > 0.0 {
        let orphans = results
            .degree
            .nodes
            .iter()
            .filter(|n| n.in_degree == 0)
            .count() as f64;
        let islands = results
            .degree
            .nodes
            .iter()
            .filter(|n| n.in_degree == 0 && n.out_degree == 0)
            .count() as f64;
        let sinks = results
            .degree
            .nodes
            .iter()
            .filter(|n| n.out_degree == 0)
            .count() as f64;

        metrics.push(Metric {
            name: "orphan_ratio".into(),
            value: orphans / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
        metrics.push(Metric {
            name: "island_ratio".into(),
            value: islands / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
        metrics.push(Metric {
            name: "sink_ratio".into(),
            value: sinks / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
    }

    // From edge classification
    let total_edges = results.edge_classification.edges.len() as f64;
    if total_edges > 0.0 {
        let broken = results
            .edge_classification
            .edges
            .iter()
            .filter(|e| matches!(e.status, EdgeStatus::Broken))
            .count() as f64;
        let external = results
            .edge_classification
            .edges
            .iter()
            .filter(|e| matches!(e.status, EdgeStatus::External))
            .count() as f64;
        let symlink = results
            .edge_classification
            .edges
            .iter()
            .filter(|e| matches!(e.status, EdgeStatus::SymlinkTarget { .. }))
            .count() as f64;

        metrics.push(Metric {
            name: "dead_link_rate".into(),
            value: broken / total_edges,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
        metrics.push(Metric {
            name: "external_link_ratio".into(),
            value: external / total_edges,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
        metrics.push(Metric {
            name: "symlink_ratio".into(),
            value: symlink / total_edges,
            kind: MetricKind::Ratio,
            dimension: "completeness".into(),
        });
    }

    metrics
}
