pub mod betweenness;
pub mod bridges;
pub mod change_propagation;
pub mod connected_components;
pub mod degree;
pub mod depth;
pub mod graph_stats;
pub mod impact_radius;
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
        "graph-stats",
        "impact-radius",
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

/// The complete, enriched graph. Built once, enriched once.
/// Carries the graph plus all structural analyses unconditionally.
pub struct EnrichedGraph {
    pub graph: Graph,
    pub betweenness: betweenness::BetweennessResult,
    pub bridges: bridges::BridgesResult,
    pub change_propagation: change_propagation::ChangePropagationResult,
    pub connected_components: connected_components::ConnectedComponentsResult,
    pub degree: degree::DegreeResult,
    pub depth: depth::DepthResult,
    pub graph_stats: graph_stats::GraphStatsResult,
    pub impact_radius: impact_radius::ImpactRadiusResult,
    pub pagerank: pagerank::PageRankResult,
    pub scc: scc::SccResult,
    pub transitive_reduction: transitive_reduction::TransitiveReductionResult,
}

/// Build an enriched graph: construct the graph, then run all analyses unconditionally.
pub fn enrich(
    root: &Path,
    config: &Config,
    lockfile: Option<&Lockfile>,
) -> anyhow::Result<EnrichedGraph> {
    let graph = crate::graph::build_graph(root, config)?;
    Ok(enrich_graph(graph, root, config, lockfile))
}

/// Enrich a pre-built graph with all analyses.
pub fn enrich_graph(
    graph: Graph,
    root: &Path,
    config: &Config,
    lockfile: Option<&Lockfile>,
) -> EnrichedGraph {
    let ctx = AnalysisContext {
        graph: &graph,
        root,
        config,
        lockfile,
    };

    let betweenness = betweenness::Betweenness.run(&ctx);
    let bridges = bridges::Bridges.run(&ctx);
    let change_propagation = change_propagation::ChangePropagation.run(&ctx);
    let connected_components = connected_components::ConnectedComponents.run(&ctx);
    let degree = degree::Degree.run(&ctx);
    let depth = depth::Depth.run(&ctx);
    let graph_stats = graph_stats::GraphStats.run(&ctx);
    let impact_radius = impact_radius::ImpactRadius.run(&ctx);
    let pagerank = pagerank::PageRank.run(&ctx);
    let scc = scc::StronglyConnectedComponents.run(&ctx);
    let transitive_reduction = transitive_reduction::TransitiveReduction.run(&ctx);

    EnrichedGraph {
        graph,
        betweenness,
        bridges,
        change_propagation,
        connected_components,
        degree,
        depth,
        graph_stats,
        impact_radius,
        pagerank,
        scc,
        transitive_reduction,
    }
}
