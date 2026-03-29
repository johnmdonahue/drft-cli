pub mod broken_link;
pub mod containment;
pub mod custom;
pub mod cycle;
pub mod directory_link;
pub mod encapsulation;
pub mod fragility;
pub mod fragmentation;
pub mod indirect_link;
pub mod layer_violation;
pub mod lockfile_outdated;
pub mod orphan;
pub mod redundant_edge;
pub mod stale;

use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use std::path::Path;

pub trait Rule {
    fn name(&self) -> &str;
    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic>;
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
        Box::new(lockfile_outdated::LockfileOutdatedRule),
        Box::new(orphan::OrphanRule),
        Box::new(redundant_edge::RedundantEdgeRule),
        Box::new(stale::StaleRule),
    ]
}
