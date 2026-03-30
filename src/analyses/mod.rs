pub mod betweenness;
pub mod bridges;
pub mod change_propagation;
pub mod connected_components;
pub mod degree;
pub mod depth;
pub mod graph_boundaries;
pub mod graph_stats;
pub mod pagerank;
pub mod scc;
pub mod transitive_reduction;

use crate::config::Config;
use crate::graph::Graph;
use crate::lockfile::Lockfile;
use std::path::Path;

/// Context passed to every analysis, providing access to the graph,
/// filesystem root, config, and optional lockfile.
pub struct AnalysisContext<'a> {
    pub graph: &'a Graph,
    pub root: &'a Path,
    pub config: &'a Config,
    pub lockfile: Option<&'a Lockfile>,
}

/// All known analysis names, sorted alphabetically.
pub fn all_analysis_names() -> &'static [&'static str] {
    &[
        "betweenness",
        "bridges",
        "change-propagation",
        "connected-components",
        "degree",
        "depth",
        "graph-boundaries",
        "graph-stats",
        "pagerank",
        "scc",
        "transitive-reduction",
    ]
}

/// An analysis computes structured data about the graph.
/// Rules consume analysis results and map them to diagnostics.
/// Metrics extract scalar values from analysis results.
/// See [`docs/analyses`](../../docs/analyses/README.md) for conceptual documentation on each analysis.
pub trait Analysis {
    type Output: serde::Serialize;

    fn name(&self) -> &str;

    fn run(&self, ctx: &AnalysisContext) -> Self::Output;
}
