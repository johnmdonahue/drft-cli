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

/// How a parser's links are turned into targets. The two parsers disagree about
/// one thing, and inheriting either answer in the other's job is a defect.
///
/// A body link written `[see](#section)` is an **intra-file anchor**: it names a
/// position in the file it sits in, so it is not an edge to anywhere and drops
/// out. A frontmatter value is a citation of *another* document — a provenance
/// claim has no "this file" form — so a value beginning with `#` names no
/// document at all. It cannot resolve, and it must not vanish either: the author
/// declared it, so it becomes a target nothing defines and `unresolved-edge`
/// reports it, exactly as it reports `TBD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPolicy {
    /// Body links: a fragment-only target is an intra-file anchor, not an edge.
    Body,
    /// Declared frontmatter values: every value names another document, so a
    /// fragment-only value is a citation of nothing rather than an anchor.
    Declared,
}

/// Resolve one discovered link into its edge target plus the **occurrence**
/// metadata recording how the author wrote it: `line`, the fragment-qualified
/// `link`, and the literal `raw` text.
///
/// Returns `None` only where there is no target to name: an empty or
/// whitespace-only value, and — under [`LinkPolicy::Body`] — an anchor-only one.
/// A fragment (`#heading`) is stripped from the target, which is the node
/// identity, and preserved on the occurrence. Non-URI targets are resolved
/// relative to `source`; URIs pass through unchanged.
///
/// Resolution that yields the empty string becomes `.` — the graph root's own
/// spelling. `..` from `docs/a.md` and `.` from `a.md` both name the root, which
/// normalizes to `""`, and an edge to `""` is a node reference the graph does not
/// contain: it would reach `drft.lock` and the JGF export as a dangling one.
///
/// `.` rather than the literal, because the literal collides. `..` from
/// `docs/a.md` names the root and `../..` names one level above it — two
/// different places — and both would render as `..`, merging two edges into one
/// finding. Keeping the resolved spelling also keeps `raw`, which the `base !=
/// target` guard would drop if the target were the literal it came from. The
/// root carries no node, so the edge is unresolved either way.
///
/// `raw` is kept only when resolution moved the path. The resolved target alone
/// cannot distinguish `foo.md` from `./foo.md` — they resolve identically — and
/// that distinction is what tells a wrong base from a deliberate doc-relative
/// link. Graph-only, like `line`; never locked.
fn occurrence(source: &str, link: &Link, policy: LinkPolicy) -> Option<(String, Metadata)> {
    let trimmed = link.target.trim();
    if trimmed.is_empty() {
        return None;
    }
    let anchor_only = trimmed.starts_with('#');
    if anchor_only && policy == LinkPolicy::Body {
        return None;
    }

    let (base, fragment) = match trimmed.find('#') {
        // A declared value opening with `#` is the whole target, not a fragment
        // qualifying a document that is not there.
        Some(_) if anchor_only => (trimmed, None),
        Some(i) => (&trimmed[..i], Some(&trimmed[i..])),
        None => (trimmed, None),
    };

    let target = if is_uri(base) || anchor_only {
        base.to_string()
    } else {
        let resolved = resolve_link(source, base);
        if resolved.is_empty() {
            ".".to_string()
        } else {
            resolved
        }
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
pub fn link_edges(source: &str, links: &[Link], policy: LinkPolicy) -> Vec<Edge> {
    let mut by_target: BTreeMap<String, Vec<Metadata>> = BTreeMap::new();
    for link in links {
        let Some((target, occurrence)) = occurrence(source, link, policy) else {
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
        link_edges(source, &[link(target, Some(1))], LinkPolicy::Body).pop()
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
        let edges = link_edges("a.md", &links, LinkPolicy::Body);
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
        let edges = link_edges("registers/work-items.md", &links, LinkPolicy::Body);
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
        let edges = link_edges("a.md", &links, LinkPolicy::Body);
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
        let edges = link_edges("docs/guide.md", &links, LinkPolicy::Body);
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
        let edges = link_edges("a.md", &[link("b.md", None)], LinkPolicy::Body);
        assert_eq!(edges.len(), 1);
        assert!(edges[0].metadata.is_empty());
    }

    // The two policies differ on exactly one input class, and the difference is
    // load-bearing in both directions: swapping either builder's policy left the
    // whole suite green until these existed.

    #[test]
    fn a_body_link_naming_only_a_fragment_is_not_an_edge() {
        // `[see](#section)` names a position in the file it sits in. It is not a
        // reference to another document, so there is nothing to draw an edge to.
        let edges = link_edges("doc.md", &[link("#section", Some(1))], LinkPolicy::Body);
        assert!(edges.is_empty(), "got: {edges:?}");
    }

    #[test]
    fn a_declared_value_naming_only_a_fragment_is_an_edge() {
        // A frontmatter value cites another document — a provenance claim has no
        // "this file" form — so `#section` names no document at all. It cannot
        // resolve, and it must not vanish: the author declared it.
        let edges = link_edges("doc.md", &[link("#section", Some(1))], LinkPolicy::Declared);
        assert_eq!(edges.len(), 1, "got: {edges:?}");
        assert_eq!(edges[0].target, "#section");
    }

    #[test]
    fn a_declared_fragment_on_a_document_still_splits() {
        // Only a value that *begins* with `#` is a whole target. A cross-document
        // fragment keeps the document as the target and the fragment on the
        // occurrence, under both policies.
        for policy in [LinkPolicy::Body, LinkPolicy::Declared] {
            let edges = link_edges("doc.md", &[link("other.md#section", Some(1))], policy);
            assert_eq!(edges.len(), 1, "{policy:?}: {edges:?}");
            assert_eq!(edges[0].target, "other.md", "{policy:?}");
        }
    }

    #[test]
    fn a_target_resolving_to_the_graph_root_is_named_dot() {
        // `..` from `docs/a.md` and `.` from `a.md` both name the root, which
        // normalizes to the empty string. An edge to `""` is a node reference the
        // graph does not contain and would reach the lockfile and the JGF export
        // as a dangling one.
        // `sub/..` from `docs/a.md` is deliberately absent: it cancels back to
        // `docs`, which is a real directory node, so it never reaches this path.
        for (source, raw) in [("docs/a.md", ".."), ("a.md", "."), ("a.md", "sub/..")] {
            let edges = link_edges(source, &[link(raw, Some(1))], LinkPolicy::Body);
            assert_eq!(edges.len(), 1, "{source} → {raw}");
            assert_eq!(edges[0].target, ".", "{source} → {raw}");
        }
    }

    #[test]
    fn the_root_and_the_level_above_it_stay_distinct() {
        // Naming the fallback after the literal would merge these: `..` names the
        // root and `../..` names one level above it, and both would render `..`.
        // Two places, two edges — and each keeps the literal the author wrote.
        let edges = link_edges(
            "docs/a.md",
            &[link("..", Some(1)), link("../..", Some(3))],
            LinkPolicy::Body,
        );
        let targets: Vec<&str> = edges.iter().map(|e| e.target.as_str()).collect();
        assert_eq!(targets, vec![".", ".."], "got: {edges:?}");
        // The literal survives on each occurrence. Naming the fallback after the
        // literal would set `target == base` and the `base != target` guard would
        // drop exactly the text the fallback exists to keep.
        for (edge, expected) in edges.iter().zip(["..", "../.."]) {
            let raw = edge.metadata["occurrences"][0]["raw"]
                .as_str()
                .unwrap_or_else(|| panic!("no raw on {edge:?}"));
            assert_eq!(raw, expected, "{edge:?}");
        }
    }
}
