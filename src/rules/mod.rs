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

use crate::analyses::EnrichedGraph;
use crate::diagnostic::Diagnostic;

/// Context passed to every rule. Rules are pure functions over the
/// enriched graph — no filesystem access, no config, no lockfile.
///
/// See [`docs/rules`](../../docs/rules/README.md) for details.
pub struct RuleContext<'a> {
    pub graph: &'a EnrichedGraph,
    /// Per-rule options from `[rules.<name>.options]`. drft passes through, rules interpret.
    pub options: Option<&'a toml::Value>,
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
