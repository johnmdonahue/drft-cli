//! Builders turn a source's `(path, bytes)` records into graph nodes and edges.
//! v0.8 ships the `fs` node builder plus two text builders (parsers):
//! `markdown` (edges) and `frontmatter` (edges + metadata).

pub mod frontmatter;
pub mod fs;
pub mod markdown;

use serde_json::Value;

use crate::model::{Edge, Metadata};
use crate::util::{is_uri, resolve_link};

/// Turn a raw link string discovered by a text builder into an edge from
/// `source` to its resolved target.
///
/// Returns `None` for links with no file target (empty or anchor-only). A
/// fragment (`#heading`) is stripped from the target — which is the node
/// identity — and preserved as the edge's `link` metadata. Non-URI targets are
/// resolved relative to `source`; URIs pass through unchanged.
pub fn link_edge(source: &str, raw: &str) -> Option<Edge> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let (base, fragment) = match trimmed.find('#') {
        Some(i) => (&trimmed[..i], Some(&trimmed[i..])),
        None => (trimmed, None),
    };
    if base.is_empty() {
        return None;
    }

    let target = if is_uri(base) {
        base.to_string()
    } else {
        resolve_link(source, base)
    };

    let mut metadata = Metadata::new();
    if let Some(frag) = fragment {
        metadata.insert("link".into(), Value::String(format!("{target}{frag}")));
    }

    Some(Edge::with_metadata(source, target, metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_target() {
        let edge = link_edge("docs/guide.md", "setup.md").unwrap();
        assert_eq!(edge.source, "docs/guide.md");
        assert_eq!(edge.target, "docs/setup.md");
        assert!(edge.metadata.is_empty());
    }

    #[test]
    fn strips_fragment_into_link_metadata() {
        let edge = link_edge("a.md", "b.md#heading").unwrap();
        assert_eq!(edge.target, "b.md");
        assert_eq!(edge.metadata["link"], Value::String("b.md#heading".into()));
    }

    #[test]
    fn passes_through_uris() {
        let edge = link_edge("a.md", "https://example.com/x").unwrap();
        assert_eq!(edge.target, "https://example.com/x");
    }

    #[test]
    fn drops_anchor_only_and_empty() {
        assert!(link_edge("a.md", "#section").is_none());
        assert!(link_edge("a.md", "   ").is_none());
    }
}
