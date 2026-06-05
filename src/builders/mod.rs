//! Builders turn a source's `(path, bytes)` records into graph nodes and edges.
//! v0.8 ships the `fs` node builder plus two text builders (parsers):
//! `markdown` (edges) and `frontmatter` (edges + metadata).

pub mod frontmatter;
pub mod fs;
pub mod markdown;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::{Edge, Metadata};
use crate::parsers::Link;
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

/// Resolve a parser's discovered links into edges, one per `(source, target)`.
///
/// Multiple links to the same target collapse to a single edge: `compose` dedups
/// by `(source, target)` and overwrites per-namespace metadata, so aggregation
/// must happen here. Source lines are unioned into a sorted, deduped `lines`
/// array (omitted entirely when no link carried a line); the first occurrence's
/// `link` (fragment) metadata wins.
pub fn link_edges(source: &str, links: &[Link]) -> Vec<Edge> {
    let mut by_target: BTreeMap<String, (Edge, BTreeSet<usize>)> = BTreeMap::new();
    for link in links {
        let Some(edge) = link_edge(source, &link.target) else {
            continue;
        };
        let entry = by_target
            .entry(edge.target.clone())
            .or_insert_with(|| (edge, BTreeSet::new()));
        if let Some(line) = link.line {
            entry.1.insert(line);
        }
    }

    by_target
        .into_values()
        .map(|(mut edge, lines)| {
            if !lines.is_empty() {
                let arr: Vec<Value> = lines.into_iter().map(|l| Value::from(l as u64)).collect();
                edge.metadata.insert("lines".into(), Value::Array(arr));
            }
            edge
        })
        .collect()
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

    fn link(target: &str, line: Option<usize>) -> Link {
        Link {
            target: target.into(),
            line,
        }
    }

    #[test]
    fn aggregates_lines_per_target() {
        // The same target linked on two lines collapses to one edge whose `lines`
        // is sorted and deduped; a distinct target is its own edge.
        let links = vec![
            link("b.md", Some(6)),
            link("b.md", Some(2)),
            link("b.md", Some(6)),
            link("c.md", Some(3)),
        ];
        let edges = link_edges("a.md", &links);
        let b = edges.iter().find(|e| e.target == "b.md").unwrap();
        assert_eq!(b.metadata["lines"], serde_json::json!([2, 6]));
        let c = edges.iter().find(|e| e.target == "c.md").unwrap();
        assert_eq!(c.metadata["lines"], serde_json::json!([3]));
    }

    #[test]
    fn omits_lines_when_no_line_known() {
        let edges = link_edges("a.md", &[link("b.md", None)]);
        assert_eq!(edges.len(), 1);
        assert!(edges[0].metadata.get("lines").is_none());
    }
}
