pub mod betweenness;
pub mod bridges;
pub mod change_propagation;
pub mod connected_components;
pub mod degree;
pub mod depth;
pub mod edge_classification;
pub mod graph_stats;
pub mod pagerank;
pub mod scc;
pub mod scope_boundaries;
pub mod transitive_reduction;

use crate::graph::Graph;
use std::path::Path;

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

/// An analysis computes structured data about the graph.
/// Rules consume analysis results and map them to diagnostics.
/// See `docs/analyses/` for conceptual documentation on each analysis.
pub trait Analysis {
    type Output: serde::Serialize;

    fn name(&self) -> &str;

    fn run(&self, graph: &Graph, root: &Path) -> Self::Output;

    /// Extract named scalar metrics from the analysis result.
    fn metrics(&self, _output: &Self::Output, _graph: &Graph) -> Vec<Metric> {
        vec![]
    }
}
