use crate::analyses::Analysis;
use crate::analyses::AnalysisContext;
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

/// Compute all scalar health metrics from the graph.
pub fn compute_metrics(ctx: &AnalysisContext, graph: &Graph) -> Vec<Metric> {
    use crate::analyses::{
        bridges::Bridges, change_propagation::ChangePropagation,
        connected_components::ConnectedComponents, degree::Degree, graph_stats::GraphStats,
        pagerank::PageRank, scc::StronglyConnectedComponents,
        transitive_reduction::TransitiveReduction,
    };

    let degree = Degree.run(ctx);
    let scc = StronglyConnectedComponents.run(ctx);
    let cc = ConnectedComponents.run(ctx);
    let stats = GraphStats.run(ctx);
    let bridges = Bridges.run(ctx);
    let reduction = TransitiveReduction.run(ctx);
    let change = ChangePropagation.run(ctx);
    let pagerank = PageRank.run(ctx);

    let mut metrics: Vec<Metric> = Vec::new();

    // Connectivity
    let total_nodes = graph
        .nodes
        .values()
        .filter(|n| graph.is_file_node(&n.path))
        .count() as f64;
    if total_nodes > 0.0 {
        let orphans = degree.nodes.iter().filter(|n| n.in_degree == 0).count() as f64;
        metrics.push(Metric {
            name: "orphan_ratio".into(),
            value: orphans / total_nodes,
            kind: MetricKind::Ratio,
            dimension: "connectivity".into(),
        });

        let islands = cc
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
        value: cc.component_count as f64,
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });
    metrics.push(Metric {
        name: "density".into(),
        value: stats.density,
        kind: MetricKind::Ratio,
        dimension: "complexity".into(),
    });
    metrics.push(Metric {
        name: "cyclomatic_complexity".into(),
        value: (graph.edges.len() as f64 - graph.nodes.len() as f64 + cc.components.len() as f64),
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });
    if let Some(d) = stats.diameter {
        metrics.push(Metric {
            name: "diameter".into(),
            value: d as f64,
            kind: MetricKind::Count,
            dimension: "complexity".into(),
        });
    }
    if let Some(avg) = stats.average_path_length {
        metrics.push(Metric {
            name: "average_path_length".into(),
            value: avg,
            kind: MetricKind::Score,
            dimension: "complexity".into(),
        });
    }

    // Conciseness
    let total_edges = graph.edges.len() as f64;
    if total_edges > 0.0 {
        metrics.push(Metric {
            name: "redundant_edge_ratio".into(),
            value: reduction.redundant_edges.len() as f64 / total_edges,
            kind: MetricKind::Ratio,
            dimension: "conciseness".into(),
        });
    }

    // Resilience
    metrics.push(Metric {
        name: "bridge_count".into(),
        value: bridges.bridges.len() as f64,
        kind: MetricKind::Count,
        dimension: "resilience".into(),
    });
    metrics.push(Metric {
        name: "cut_node_count".into(),
        value: bridges.cut_vertices.len() as f64,
        kind: MetricKind::Count,
        dimension: "resilience".into(),
    });

    // Freshness
    if change.has_lockfile {
        metrics.push(Metric {
            name: "directly_changed_count".into(),
            value: change.directly_changed.len() as f64,
            kind: MetricKind::Count,
            dimension: "freshness".into(),
        });
        metrics.push(Metric {
            name: "transitively_stale_count".into(),
            value: change.transitively_stale.len() as f64,
            kind: MetricKind::Count,
            dimension: "freshness".into(),
        });
        if total_nodes > 0.0 {
            let stale = (change.directly_changed.len() + change.transitively_stale.len()) as f64;
            metrics.push(Metric {
                name: "stale_ratio".into(),
                value: stale / total_nodes,
                kind: MetricKind::Ratio,
                dimension: "freshness".into(),
            });
        }
    }

    // PageRank concentration
    if !pagerank.nodes.is_empty() {
        let max = pagerank
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
        value: scc.nontrivial_count as f64,
        kind: MetricKind::Count,
        dimension: "complexity".into(),
    });

    metrics
}
