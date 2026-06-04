//! v0.8 rules over the composed graph: staleness (drift vs the lockfile) and
//! structural findings, orchestrated by [`check`].

pub mod check;
pub mod staleness;
pub mod structural;

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
