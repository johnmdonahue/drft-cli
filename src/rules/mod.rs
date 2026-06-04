pub mod custom;
pub mod directed_cycle;
pub mod fragmentation;
pub mod orphan_node;
pub mod schema_violation;
pub mod stale;
pub mod symlink_edge;
pub mod unresolved_edge;

// v0.8 rules over the composed graph.
pub mod check;
pub mod staleness;
pub mod structural;

use crate::analyses::EnrichedGraph;
use crate::diagnostic::Diagnostic;
use crate::model::{Edge, Metadata, Node, PROVENANCE_KEY};

/// A composed node's current `fs` content hash, if it has one.
pub(crate) fn fs_hash(node: &Node) -> Option<&str> {
    node.metadata.get("@fs")?.get("hash")?.as_str()
}

/// Whether a composed node is resolved — present with an `@fs` block. Resolution
/// is namespace presence.
pub(crate) fn is_resolved(node: &Node) -> bool {
    node.metadata.contains_key("@fs")
}

/// The `_graphs` provenance list from a node's or edge's metadata.
pub(crate) fn provenance(metadata: &Metadata) -> Vec<String> {
    metadata
        .get(PROVENANCE_KEY)
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| e.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The `_graphs` provenance list for an edge.
pub(crate) fn edge_provenance(edge: &Edge) -> Vec<String> {
    provenance(&edge.metadata)
}

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
        Box::new(directed_cycle::DirectedCycleRule),
        Box::new(fragmentation::FragmentationRule),
        Box::new(orphan_node::OrphanNodeRule),
        Box::new(schema_violation::SchemaViolationRule),
        Box::new(stale::StaleRule),
        Box::new(symlink_edge::SymlinkEdgeRule),
        Box::new(unresolved_edge::UnresolvedEdgeRule),
    ]
}
