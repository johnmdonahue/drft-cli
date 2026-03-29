use super::{AnalysisResults, Metric, MetricKind};

pub fn extract(results: &AnalysisResults) -> Vec<Metric> {
    let total_edges = results.graph.edges.len();
    let redundant = results.transitive_reduction.redundant_edges.len();
    let ratio = if total_edges > 0 {
        redundant as f64 / total_edges as f64
    } else {
        0.0
    };

    vec![Metric {
        name: "redundant_edge_ratio".into(),
        value: ratio,
        kind: MetricKind::Ratio,
        dimension: "conciseness".into(),
    }]
}
