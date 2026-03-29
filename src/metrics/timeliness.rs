use super::{AnalysisResults, Metric, MetricKind};

pub fn extract(results: &AnalysisResults) -> Vec<Metric> {
    if !results.change_propagation.has_lockfile {
        return vec![];
    }

    vec![
        Metric {
            name: "directly_changed_count".into(),
            value: results.change_propagation.directly_changed.len() as f64,
            kind: MetricKind::Count,
            dimension: "timeliness".into(),
        },
        Metric {
            name: "transitively_stale_count".into(),
            value: results.change_propagation.transitively_stale.len() as f64,
            kind: MetricKind::Count,
            dimension: "timeliness".into(),
        },
        Metric {
            name: "boundary_change_count".into(),
            value: results.change_propagation.boundary_changes.len() as f64,
            kind: MetricKind::Count,
            dimension: "timeliness".into(),
        },
    ]
}
