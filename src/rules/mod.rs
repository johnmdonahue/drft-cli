pub mod broken_link;
pub mod containment;
pub mod cycle;
pub mod directory_link;
pub mod encapsulation;
pub mod fragility;
pub mod fragmentation;
pub mod indirect_link;
pub mod layer_violation;
pub mod orphan;
pub mod redundant_edge;
pub mod script;
pub mod stale;

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
        Box::new(broken_link::BrokenLinkRule),
        Box::new(containment::ContainmentRule),
        Box::new(cycle::CycleRule),
        Box::new(directory_link::DirectoryLinkRule),
        Box::new(encapsulation::EncapsulationRule),
        Box::new(fragility::FragilityRule),
        Box::new(fragmentation::FragmentationRule),
        Box::new(indirect_link::IndirectLinkRule),
        Box::new(layer_violation::LayerViolationRule),
        Box::new(orphan::OrphanRule),
        Box::new(redundant_edge::RedundantEdgeRule),
        Box::new(stale::StaleRule),
    ]
}
