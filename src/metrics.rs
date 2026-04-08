use crate::analyses::{
    bridges::BridgesResult, change_propagation::ChangePropagationResult,
    connected_components::ConnectedComponentsResult, degree::DegreeResult,
    graph_stats::GraphStatsResult, pagerank::PageRankResult, scc::SccResult,
    transitive_reduction::TransitiveReductionResult,
};
use crate::graph::Graph;

/// A scalar metric extracted from analysis results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub kind: MetricKind,
    pub dimension: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKind {
    Ratio,
    Count,
    Score,
}

/// All known metric names, sorted alphabetically.
pub fn all_metric_names() -> &'static [&'static str] {
    &[
        "average_path_length",
        "bridge_count",
        "component_count",
        "cut_node_count",
        "cycle_count",
        "cyclomatic_complexity",
        "density",
        "diameter",
        "directly_changed_count",
        "island_ratio",
        "max_pagerank",
        "orphan_ratio",
        "redundant_edge_ratio",
        "stale_ratio",
        "transitively_stale_count",
    ]
}

/// Pre-computed analysis results needed for metric extraction.
pub struct AnalysisInputs<'a> {
    pub degree: &'a DegreeResult,
    pub scc: &'a SccResult,
    pub connected_components: &'a ConnectedComponentsResult,
    pub graph_stats: &'a GraphStatsResult,
    pub bridges: &'a BridgesResult,
    pub transitive_reduction: &'a TransitiveReductionResult,
    pub change_propagation: &'a ChangePropagationResult,
    pub pagerank: &'a PageRankResult,
}

/// Compute all scalar health metrics from pre-computed analysis results.
pub fn compute_metrics(inputs: &AnalysisInputs, graph: &Graph) -> Vec<Metric> {
    let mut metrics: Vec<Metric> = Vec::new();

    // Connectivity
    let total_nodes = graph
        .nodes
        .values()
        .filter(|n| graph.is_included_node(&n.path))
        .count() as f64;
    if total_nodes > 0.0 {
        let orphans = inputs
            .degree
            .nodes
            .iter()
            .filter(|n| n.in_degree == 0)
            .count() as f64;
        metrics.push(Metric {
            name: "orphan_ratio".into(),
            value: orphans / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "connectivity".into(),
        });

        let islands = inputs
            .connected_components
            .components
            .iter()
            .filter(|c| c.members.len() == 1)
            .count() as f64;
        metrics.push(Metric {
            name: "island_ratio".into(),
            value: islands / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "connectivity".into(),
        });
    }

    // Complexity
    metrics.push(Metric {
        name: "component_count".into(),
        value: inputs.connected_components.component_count as f64,
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });
    metrics.push(Metric {
        name: "density".into(),
        value: inputs.graph_stats.density,
        kind: MetricKind::Ratio,
        dimension: "complexity".into(),
    });
    let internal_edge_count = graph
        .edges
        .iter()
        .filter(|e| graph.is_internal_edge(e))
        .count();
    metrics.push(Metric {
        name: "cyclomatic_complexity".into(),
        value: (internal_edge_count as f64 - total_nodes
            + inputs.connected_components.components.len() as f64),
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });
    if let Some(d) = inputs.graph_stats.diameter {
        metrics.push(Metric {
            name: "diameter".into(),
            value: d as f64,
            kind: MetricKind::Count,
            dimension: "complexity".into(),
        });
    }
    if let Some(avg) = inputs.graph_stats.average_path_length {
        metrics.push(Metric {
            name: "average_path_length".into(),
            value: avg,
            kind: MetricKind::Score,
            dimension: "complexity".into(),
        });
    }

    // Conciseness
    let total_edges = internal_edge_count as f64;
    if total_edges > 0.0 {
        metrics.push(Metric {
            name: "redundant_edge_ratio".into(),
            value: inputs.transitive_reduction.redundant_edges.len() as f64 / total_edges,
            kind: MetricKind::Ratio,
            dimension: "conciseness".into(),
        });
    }

    // Resilience
    metrics.push(Metric {
        name: "bridge_count".into(),
        value: inputs.bridges.bridges.len() as f64,
        kind: MetricKind::Count,
        dimension: "resilience".into(),
    });
    metrics.push(Metric {
        name: "cut_node_count".into(),
        value: inputs.bridges.cut_vertices.len() as f64,
        kind: MetricKind::Count,
        dimension: "resilience".into(),
    });

    // Freshness
    if inputs.change_propagation.has_lockfile {
        metrics.push(Metric {
            name: "directly_changed_count".into(),
            value: inputs.change_propagation.directly_changed.len() as f64,
            kind: MetricKind::Count,
            dimension: "freshness".into(),
        });
        metrics.push(Metric {
            name: "transitively_stale_count".into(),
            value: inputs.change_propagation.transitively_stale.len() as f64,
            kind: MetricKind::Count,
            dimension: "freshness".into(),
        });
        if total_nodes > 0.0 {
            let stale = (inputs.change_propagation.directly_changed.len()
                + inputs.change_propagation.transitively_stale.len())
                as f64;
            metrics.push(Metric {
                name: "stale_ratio".into(),
                value: stale / total_nodes,
                kind: MetricKind::Ratio,
                dimension: "freshness".into(),
            });
        }
    }

    // PageRank concentration
    if !inputs.pagerank.nodes.is_empty() {
        let max = inputs
            .pagerank
            .nodes
            .iter()
            .map(|n| n.score)
            .fold(f64::NEG_INFINITY, f64::max);
        metrics.push(Metric {
            name: "max_pagerank".into(),
            value: max,
            kind: MetricKind::Score,
            dimension: "complexity".into(),
        });
    }

    // SCC
    metrics.push(Metric {
        name: "cycle_count".into(),
        value: inputs.scc.nontrivial_count as f64,
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });

    metrics
}
