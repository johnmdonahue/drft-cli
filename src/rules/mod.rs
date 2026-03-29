pub mod broken_link;
pub mod containment;
pub mod custom;
pub mod cycle;
pub mod directory_link;
pub mod encapsulation;
pub mod indirect_link;
pub mod lockfile_outdated;
pub mod orphan;
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
        Box::new(indirect_link::IndirectLinkRule),
        Box::new(lockfile_outdated::LockfileOutdatedRule),
        Box::new(orphan::OrphanRule),
        Box::new(stale::StaleRule),
    ]
}
