pub mod betweenness;
pub mod bridges;
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

/// An analysis computes structured data about the graph.
/// Rules consume analysis results and map them to diagnostics.
/// See `docs/analyses/` for conceptual documentation on each analysis.
pub trait Analysis {
    type Output: serde::Serialize;

    fn name(&self) -> &str;

    fn run(&self, graph: &Graph, root: &Path) -> Self::Output;
}
