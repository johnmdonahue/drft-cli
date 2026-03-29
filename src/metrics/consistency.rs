use super::{AnalysisResults, Metric, MetricKind, gini_coefficient};

pub fn extract(results: &AnalysisResults) -> Vec<Metric> {
    let mut metrics = Vec::new();

    // From connected components
    let total_nodes: usize = results
        .connected_components
        .components
        .iter()
        .map(|c| c.members.len())
        .sum();
    let largest = results
        .connected_components
        .components
        .first()
        .map(|c| c.members.len())
        .unwrap_or(0);
    let frag = if total_nodes > 0 {
        1.0 - (largest as f64 / total_nodes as f64)
    } else {
        0.0
    };
    metrics.push(Metric {
        name: "component_count".into(),
        value: results.connected_components.component_count as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "fragmentation_index".into(),
        value: frag,
        kind: MetricKind::Ratio,
        dimension: "consistency".into(),
    });

    // From graph stats
    metrics.push(Metric {
        name: "node_count".into(),
        value: results.graph_stats.node_count as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "edge_count".into(),
        value: results.graph_stats.edge_count as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "density".into(),
        value: results.graph_stats.density,
        kind: MetricKind::Ratio,
        dimension: "consistency".into(),
    });
    if let Some(d) = results.graph_stats.diameter {
        metrics.push(Metric {
            name: "diameter".into(),
            value: d as f64,
            kind: MetricKind::Count,
            dimension: "consistency".into(),
        });
    }
    if let Some(a) = results.graph_stats.average_path_length {
        metrics.push(Metric {
            name: "avg_path_length".into(),
            value: a,
            kind: MetricKind::Score,
            dimension: "consistency".into(),
        });
    }

    // From depth
    metrics.push(Metric {
        name: "max_depth".into(),
        value: results.depth.max_depth as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });

    // From SCC
    let v = results
        .graph
        .nodes
        .keys()
        .filter(|p| results.graph.is_real_node(p))
        .count();
    let e = results
        .graph
        .edges
        .iter()
        .filter(|e| results.graph.is_real_node(&e.source) && results.graph.is_real_node(&e.target))
        .count();
    let c = results.scc.scc_count;
    let cyclomatic = (e as i64) - (v as i64) + 2 * (c as i64);
    metrics.push(Metric {
        name: "nontrivial_scc_count".into(),
        value: results.scc.nontrivial_count as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "cyclomatic_complexity".into(),
        value: cyclomatic.max(0) as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });

    // From bridges
    metrics.push(Metric {
        name: "cut_vertex_count".into(),
        value: results.bridges.cut_vertices.len() as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "bridge_count".into(),
        value: results.bridges.bridges.len() as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });

    // From betweenness
    if !results.betweenness.nodes.is_empty() {
        let max = results
            .betweenness
            .nodes
            .iter()
            .map(|n| n.score)
            .fold(0.0f64, f64::max);
        let gini = gini_coefficient(
            &results
                .betweenness
                .nodes
                .iter()
                .map(|n| n.score)
                .collect::<Vec<_>>(),
        );
        metrics.push(Metric {
            name: "max_betweenness".into(),
            value: max,
            kind: MetricKind::Score,
            dimension: "consistency".into(),
        });
        metrics.push(Metric {
            name: "betweenness_gini".into(),
            value: gini,
            kind: MetricKind::Ratio,
            dimension: "consistency".into(),
        });
    }

    // From PageRank
    if !results.pagerank.nodes.is_empty() {
        let max = results
            .pagerank
            .nodes
            .iter()
            .map(|n| n.score)
            .fold(0.0f64, f64::max);
        let gini = gini_coefficient(
            &results
                .pagerank
                .nodes
                .iter()
                .map(|n| n.score)
                .collect::<Vec<_>>(),
        );
        metrics.push(Metric {
            name: "max_pagerank".into(),
            value: max,
            kind: MetricKind::Score,
            dimension: "consistency".into(),
        });
        metrics.push(Metric {
            name: "pagerank_gini".into(),
            value: gini,
            kind: MetricKind::Ratio,
            dimension: "consistency".into(),
        });
    }

    // From scope boundaries
    metrics.push(Metric {
        name: "escape_count".into(),
        value: results.scope_boundaries.escapes.len() as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });
    metrics.push(Metric {
        name: "encapsulation_violation_count".into(),
        value: results.scope_boundaries.encapsulation_violations.len() as f64,
        kind: MetricKind::Count,
        dimension: "consistency".into(),
    });

    metrics
}
