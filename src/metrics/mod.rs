pub mod completeness;
pub mod conciseness;
pub mod consistency;
pub mod custom;
pub mod timeliness;

use crate::analyses::betweenness::BetweennessResult;
use crate::analyses::bridges::BridgesResult;
use crate::analyses::change_propagation::ChangePropagationResult;
use crate::analyses::connected_components::ConnectedComponentsResult;
use crate::analyses::degree::DegreeResult;
use crate::analyses::depth::DepthResult;
use crate::analyses::edge_classification::EdgeClassificationResult;
use crate::analyses::graph_stats::GraphStatsResult;
use crate::analyses::pagerank::PageRankResult;
use crate::analyses::scc::SccResult;
use crate::analyses::scope_boundaries::ScopeBoundariesResult;
use crate::analyses::transitive_reduction::TransitiveReductionResult;
use crate::graph::Graph;

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

/// All analysis results needed to extract the full set of metrics.
pub struct AnalysisResults<'a> {
    pub betweenness: &'a BetweennessResult,
    pub bridges: &'a BridgesResult,
    pub change_propagation: &'a ChangePropagationResult,
    pub connected_components: &'a ConnectedComponentsResult,
    pub degree: &'a DegreeResult,
    pub depth: &'a DepthResult,
    pub edge_classification: &'a EdgeClassificationResult,
    pub graph_stats: &'a GraphStatsResult,
    pub pagerank: &'a PageRankResult,
    pub scc: &'a SccResult,
    pub scope_boundaries: &'a ScopeBoundariesResult,
    pub transitive_reduction: &'a TransitiveReductionResult,
    pub graph: &'a Graph,
}

/// Extract all metrics from a complete set of analysis results.
pub fn collect_all(results: &AnalysisResults) -> Vec<Metric> {
    let mut metrics = Vec::new();
    metrics.extend(completeness::extract(results));
    metrics.extend(consistency::extract(results));
    metrics.extend(conciseness::extract(results));
    metrics.extend(timeliness::extract(results));
    metrics
}

/// Gini coefficient for a set of values. Returns 0.0 for empty or all-zero inputs.
pub fn gini_coefficient(values: &[f64]) -> f64 {
    let n = values.len();
    if n == 0 {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum: f64 = sorted.iter().sum();
    if sum == 0.0 {
        return 0.0;
    }
    let mut numerator = 0.0;
    for (i, &v) in sorted.iter().enumerate() {
        numerator += (2.0 * (i + 1) as f64 - n as f64 - 1.0) * v;
    }
    numerator / (n as f64 * sum)
}
