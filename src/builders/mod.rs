//! Builders turn a source's `(path, bytes)` records into graph nodes and edges.
//! v0.8 ships the `fs` node builder plus two text builders (parsers):
//! `markdown` (edges + the anchors a file answers to) and `frontmatter`
//! (edges + metadata).

pub mod frontmatter;
pub mod fs;
pub mod markdown;

use std::collections::BTreeMap;

use serde_json::Value;

use crate::model::{Edge, Metadata};
use crate::parsers::Link;
use crate::util::{is_uri, resolve_link};

/// Resolve one discovered link into its edge target plus the **occurrence**
/// metadata recording how the author wrote it: `line`, the fragment-qualified
/// `link`, and the literal `raw` text.
///
/// Returns `None` for links with no file target (empty or anchor-only). A
/// fragment (`#heading`) is stripped from the target — which is the node
/// identity — and preserved on the occurrence. Non-URI targets are resolved
/// relative to `source`; URIs pass through unchanged.
///
/// `raw` is kept only when resolution moved the path. The resolved target alone
/// cannot distinguish `foo.md` from `./foo.md` — they resolve identically — and
/// that distinction is what tells a wrong base from a deliberate doc-relative
/// link. Graph-only, like `line`; never locked.
fn occurrence(source: &str, link: &Link) -> Option<(String, Metadata)> {
    let trimmed = link.target.trim();
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

    let mut occurrence = Metadata::new();
    if let Some(line) = link.line {
        occurrence.insert("line".into(), Value::from(line as u64));
    }
    if let Some(frag) = fragment {
        occurrence.insert("link".into(), Value::String(format!("{target}{frag}")));
    }
    if base != target {
        occurrence.insert("raw".into(), Value::String(base.to_string()));
    }

    Some((target, occurrence))
}

/// Resolve a parser's discovered links into edges, one per `(source, target)`.
///
/// Multiple links to the same target collapse to a single edge: `compose` dedups
/// by `(source, target)` and overwrites per-namespace metadata, so aggregation
/// must happen here. Each link contributes its own entry to the edge's
/// `occurrences` array, sorted by line.
///
/// Per-occurrence is the point. A source citing two anchors of one target — six
/// lines naming `#security-console` and one naming `#ngwaf-edge` — is one edge
/// with two spellings, and a scalar `link` would attribute the first to all
/// seven. Identical occurrences dedup; distinct ones both survive.
pub fn link_edges(source: &str, links: &[Link]) -> Vec<Edge> {
    let mut by_target: BTreeMap<String, Vec<Metadata>> = BTreeMap::new();
    for link in links {
        let Some((target, occurrence)) = occurrence(source, link) else {
            continue;
        };
        // An occurrence with nothing known about it carries no information, so
        // it stays out and the edge keeps today's no-metadata shape.
        if occurrence.is_empty() {
            by_target.entry(target).or_default();
            continue;
        }
        let entry = by_target.entry(target).or_default();
        if !entry.contains(&occurrence) {
            entry.push(occurrence);
        }
    }

    by_target
        .into_iter()
        .map(|(target, mut occurrences)| {
            let mut metadata = Metadata::new();
            if !occurrences.is_empty() {
                // Stable sort on the line, with line-less occurrences last, so
                // the array reads in source order.
                occurrences
                    .sort_by_key(|o| o.get("line").and_then(Value::as_u64).unwrap_or(u64::MAX));
                metadata.insert(
                    "occurrences".into(),
                    Value::Array(occurrences.into_iter().map(Value::Object).collect()),
                );
            }
            Edge::with_metadata(source, target, metadata)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn link(target: &str, line: Option<usize>) -> Link {
        Link {
            target: target.into(),
            line,
        }
    }

    /// The single edge `link_edges` produces for one link, for the resolution tests.
    fn one(source: &str, target: &str) -> Option<Edge> {
        link_edges(source, &[link(target, Some(1))]).pop()
    }

    #[test]
    fn resolves_relative_target() {
        let edge = one("docs/guide.md", "setup.md").unwrap();
        assert_eq!(edge.source, "docs/guide.md");
        assert_eq!(edge.target, "docs/setup.md");
        // Resolution moved the path, so the literal text is kept for diagnostics.
        assert_eq!(
            edge.metadata["occurrences"],
            json!([{ "line": 1, "raw": "setup.md" }])
        );
    }

    #[test]
    fn no_raw_metadata_when_resolution_is_identity() {
        // A link already written from the graph root resolves to itself — there is
        // no second spelling worth recording.
        let edge = one("guide.md", "setup.md").unwrap();
        assert_eq!(edge.target, "setup.md");
        assert_eq!(edge.metadata["occurrences"], json!([{ "line": 1 }]));
    }

    #[test]
    fn strips_fragment_into_link_metadata() {
        let edge = one("a.md", "b.md#heading").unwrap();
        assert_eq!(edge.target, "b.md");
        assert_eq!(
            edge.metadata["occurrences"],
            json!([{ "line": 1, "link": "b.md#heading" }])
        );
    }

    #[test]
    fn passes_through_uris() {
        let edge = one("a.md", "https://example.com/x").unwrap();
        assert_eq!(edge.target, "https://example.com/x");
    }

    #[test]
    fn drops_anchor_only_and_empty() {
        assert!(one("a.md", "#section").is_none());
        assert!(one("a.md", "   ").is_none());
    }

    #[test]
    fn aggregates_occurrences_per_target() {
        // The same target linked on two lines collapses to one edge whose
        // occurrences are sorted and deduped; a distinct target is its own edge.
        let links = vec![
            link("b.md", Some(6)),
            link("b.md", Some(2)),
            link("b.md", Some(6)),
            link("c.md", Some(3)),
        ];
        let edges = link_edges("a.md", &links);
        let b = edges.iter().find(|e| e.target == "b.md").unwrap();
        assert_eq!(
            b.metadata["occurrences"],
            json!([{ "line": 2 }, { "line": 6 }])
        );
        let c = edges.iter().find(|e| e.target == "c.md").unwrap();
        assert_eq!(c.metadata["occurrences"], json!([{ "line": 3 }]));
    }

    #[test]
    fn each_occurrence_keeps_its_own_fragment() {
        // The defect per-occurrence metadata exists to fix: two anchors of one
        // target must not collapse to whichever spelling came first.
        let links = vec![
            link("./owners.md#security-console", Some(53)),
            link("./owners.md#ngwaf-edge", Some(77)),
            link("./owners.md#security-console", Some(89)),
        ];
        let edges = link_edges("registers/work-items.md", &links);
        assert_eq!(edges.len(), 1, "one edge, three occurrences");
        assert_eq!(
            edges[0].metadata["occurrences"],
            json!([
                { "line": 53, "link": "registers/owners.md#security-console", "raw": "./owners.md" },
                { "line": 77, "link": "registers/owners.md#ngwaf-edge", "raw": "./owners.md" },
                { "line": 89, "link": "registers/owners.md#security-console", "raw": "./owners.md" },
            ])
        );
    }

    #[test]
    fn each_occurrence_keeps_its_own_spelling() {
        // Two spellings that resolve identically are still two facts about the
        // source; the edge records both.
        let links = vec![link("./b.md", Some(4)), link("b.md", Some(9))];
        let edges = link_edges("a.md", &links);
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0].metadata["occurrences"],
            json!([{ "line": 4, "raw": "./b.md" }, { "line": 9 }])
        );
    }

    #[test]
    fn two_spellings_both_moved_by_resolution_keep_both_raws() {
        // The aggregation the `unresolved-edge` wrong-base cause rides on: two
        // spellings resolving to one target, each differing from it, must both
        // survive so a rule scanning the occurrences can reach the second. A
        // first-occurrence-wins aggregation would keep only `./config.md`.
        let links = vec![link("./config.md", Some(3)), link("config.md", Some(5))];
        let edges = link_edges("docs/guide.md", &links);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].target, "docs/config.md");
        assert_eq!(
            edges[0].metadata["occurrences"],
            json!([
                { "line": 3, "raw": "./config.md" },
                { "line": 5, "raw": "config.md" },
            ])
        );
    }

    #[test]
    fn omits_occurrences_when_nothing_is_known() {
        // A parser that cannot locate its link contributes no facts, so the edge
        // keeps the bare no-metadata shape.
        let edges = link_edges("a.md", &[link("b.md", None)]);
        assert_eq!(edges.len(), 1);
        assert!(edges[0].metadata.is_empty());
    }
}
