//! v0.8 rules over the composed graph: staleness (drift vs the lockfile) and
//! structural findings, orchestrated by [`check`].

pub mod check;
pub mod staleness;
pub mod structural;

use crate::model::{Edge, Metadata, PROVENANCE_KEY};

/// A hash truncated for display in findings: the `b3:` prefix plus the first 10
/// hex chars. Comparisons always use the full hash; this is cosmetic.
pub(crate) fn short_hash(hash: &str) -> &str {
    &hash[..hash.len().min(13)]
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
