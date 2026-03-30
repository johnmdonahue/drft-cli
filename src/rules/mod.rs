pub mod boundary_violation;
pub mod dangling_edge;
pub mod directed_cycle;
pub mod directory_edge;
pub mod encapsulation_violation;
pub mod fragility;
pub mod fragmentation;
pub mod layer_violation;
pub mod orphan_node;
pub mod redundant_edge;
pub mod script;
pub mod stale;
pub mod symlink_edge;

use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::lockfile::Lockfile;
use std::path::Path;

/// Context passed to every rule, providing access to the graph,
/// filesystem root, config, and optional lockfile.
///
/// See [`docs/rules`](../../docs/rules/README.md) for details.
pub struct RuleContext<'a> {
    pub graph: &'a Graph,
    pub root: &'a Path,
    pub config: &'a Config,
    pub lockfile: Option<&'a Lockfile>,
}

pub trait Rule {
    fn name(&self) -> &str;
    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic>;
}

pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(boundary_violation::BoundaryViolationRule),
        Box::new(dangling_edge::DanglingEdgeRule),
        Box::new(directed_cycle::DirectedCycleRule),
        Box::new(directory_edge::DirectoryEdgeRule),
        Box::new(encapsulation_violation::EncapsulationViolationRule),
        Box::new(fragility::FragilityRule),
        Box::new(fragmentation::FragmentationRule),
        Box::new(layer_violation::LayerViolationRule),
        Box::new(orphan_node::OrphanNodeRule),
        Box::new(redundant_edge::RedundantEdgeRule),
        Box::new(stale::StaleRule),
        Box::new(symlink_edge::SymlinkEdgeRule),
    ]
}
