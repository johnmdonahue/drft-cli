//! Structural rules: findings derived from graph shape alone, no lockfile.
//!
//! Findings: `unresolved-edge` (an edge target with no `@fs` block — no defining
//! node), `unresolved-fragment` (a link's `#fragment` names no anchor the target
//! defines), and `detached-node` (a node with no inbound or outbound edges). URI
//! targets are intentional external references, not unresolved.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::Value;

use crate::diagnostic::Finding;
use crate::model::{Graph, Node};
use crate::rules::{edge_provenance, provenance};
use crate::util::is_uri;

/// Evaluate structural findings for `graph`. `anchor_namespaces` names the
/// graphs whose parser publishes the addresses a fragment may be checked
/// against; with none, `unresolved-fragment` cannot fire.
pub fn evaluate(graph: &Graph, anchor_namespaces: &[String]) -> Vec<Finding> {
    let mut findings = evaluate_edges(
        graph,
        &graph.edges.iter().collect::<Vec<_>>(),
        anchor_namespaces,
    );
    // detached-node: a file touched by no edge in either direction. Directories
    // are structural scaffolding — links point at the files inside them, not at
    // the directory — so a link-less directory is normal, not orphaned content.
    let mut connected: HashSet<&str> = HashSet::new();
    for edge in &graph.edges {
        connected.insert(edge.source.as_str());
        connected.insert(edge.target.as_str());
    }
    for (path, node) in &graph.nodes {
        if node.fs_type() == Some("directory") {
            continue;
        }
        if !connected.contains(path.as_str()) {
            findings.push(Finding::warn(
                "detached-node",
                path,
                provenance(&node.metadata),
                "no connections",
            ));
        }
    }

    findings
}

/// Evaluate only these edges, retaining the full graph for target resolution.
/// Edge selection precedes fragment rendering so literal `#` paths retain identity.
pub fn evaluate_edges(
    graph: &Graph,
    edges: &[&crate::model::Edge],
    anchor_namespaces: &[String],
) -> Vec<Finding> {
    let mut findings = Vec::new();

    // unresolved-edge: a non-URI edge target with no defining node.
    for edge in edges {
        if is_uri(&edge.target) {
            continue;
        }
        let resolved = graph.nodes.get(&edge.target).is_some_and(Node::is_resolved);
        if !resolved {
            let mut finding = Finding::warn(
                "unresolved-edge",
                &edge.source,
                edge_provenance(edge),
                "no defining node",
            )
            .with_target(&edge.target)
            .with_lines(edge.lines());
            if let Some(cause) = wrong_base_cause(graph, edge) {
                finding = finding.with_cause(cause);
            }
            findings.push(finding);
        }
    }

    // unresolved-fragment: a link carries a `#fragment` the target does not
    // define. Reported per fragment rather than per edge, because a source citing
    // two anchors of one target is one edge with two claims and only one of them
    // may be wrong.
    for edge in edges {
        if is_uri(&edge.target) {
            continue;
        }
        // An unresolvable target is already reported as `unresolved-edge`; the
        // fragment is the lesser half of the same mistake.
        let Some(target) = graph.nodes.get(&edge.target).filter(|n| n.is_resolved()) else {
            continue;
        };
        // `None` means nothing read the target as a document with addressable
        // positions, so its fragments are unknown, not broken.
        let Some(anchors) = target.anchors(anchor_namespaces) else {
            continue;
        };

        let mut missing: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        for occurrence in edge.occurrences() {
            let Some(link) = occurrence.get("link").and_then(Value::as_str) else {
                continue;
            };
            // A bare `#` is a link to the top of the page, which every document
            // answers to.
            let Some(fragment) = link
                .split_once('#')
                .map(|(_, f)| f)
                .filter(|f| !f.is_empty())
            else {
                continue;
            };
            // A browser percent-decodes the fragment before looking for an id,
            // and GitHub's own copied permalink is percent-encoded for any
            // non-ASCII anchor. Decoding accepts exactly what the platform
            // accepts; it is not the loosening that slugging the fragment would
            // be, since `#OBS%2092` still decodes to `OBS 92` and still misses.
            let decoded = percent_decode(fragment);
            if anchors.contains(&fragment) || anchors.iter().any(|a| *a == decoded) {
                continue;
            }
            let lines = missing.entry(fragment).or_default();
            if let Some(line) = occurrence.get("line").and_then(Value::as_u64) {
                lines.insert(line as usize);
            }
        }

        for (fragment, lines) in missing {
            let mut finding = Finding::warn(
                "unresolved-fragment",
                &edge.source,
                edge_provenance(edge),
                "no matching anchor",
            )
            .with_target(format!("{}#{fragment}", edge.target))
            .with_lines(lines.into_iter().collect());
            if let Some(cause) = case_mismatch_cause(&anchors, fragment, &percent_decode(fragment))
            {
                finding = finding.with_cause(cause);
            }
            findings.push(finding);
        }
    }

    findings
}

/// Percent-decode a fragment, resolving `%XX` byte escapes as UTF-8. Invalid
/// escapes and invalid UTF-8 leave the text as written, which then simply fails
/// to match — a fragment drft cannot decode is not one it should guess at.
fn percent_decode(fragment: &str) -> String {
    let bytes = fragment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = fragment.get(i + 1..i + 3)
            // `from_str_radix` accepts a leading sign, so `%+A` would decode.
            && hex.bytes().all(|b| b.is_ascii_hexdigit())
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| fragment.to_string())
}

/// A fragment that matches an anchor once case is ignored is almost certainly a
/// typo, so name the anchor it meant.
///
/// Id matching is case-sensitive, so this link is broken rather than merely
/// untidy — the cause exists to make the fix obvious, not to excuse it.
/// Resolution stays exact: matching case-insensitively here would accept an
/// address the platform does not resolve.
fn case_mismatch_cause(anchors: &[&str], fragment: &str, decoded: &str) -> Option<String> {
    // Checked against the decoded spelling too, so a percent-encoded fragment
    // gets the same help as a literal one.
    let written = fragment.to_lowercase();
    let decoded = decoded.to_lowercase();
    let anchor = anchors.iter().find(|anchor| {
        let anchor = anchor.to_lowercase();
        anchor == written || anchor == decoded
    })?;
    Some(format!(
        "`#{fragment}` differs only in case from `#{anchor}`, which the target defines"
    ))
}

/// A path written against the graph root when links resolve against the
/// declaring file reads as a typo — the reported target is a path the author
/// never wrote. Name the cause when the literal text resolves from the root.
///
/// Gated on the raw text carrying no explicit `./`, `../` or `/` prefix: those
/// are unambiguously relative by intent, so a root file of the same name is a
/// coincidence rather than the mistake. That leaves the bare-path case, where a
/// hit is all but certainly a wrong base.
fn wrong_base_cause(graph: &Graph, edge: &crate::model::Edge) -> Option<String> {
    let raw = edge.raw_links().into_iter().find(|raw| {
        !(raw.starts_with("./") || raw.starts_with("../") || raw.starts_with('/'))
            && graph.nodes.get(*raw).is_some_and(Node::is_resolved)
    })?;
    let suggestion = crate::util::relative_from(&edge.source, raw);
    // Not escaped here. A cause is data on the finding and is serialised to JSON,
    // where the value has to survive exactly as written; escaping belongs to the
    // text rendering, which applies it in `Finding::format_text`.
    Some(format!(
        "`{raw}` resolves from the graph root, but paths resolve relative to the declaring file (did you mean `{suggestion}`?)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::compose;
    use crate::model::{Edge, GraphSet, Metadata, Node};
    use serde_json::json;

    fn fs_node() -> Node {
        Node::new(
            json!({ "type": "file", "hash": "b3:x" })
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    fn names(findings: &[Finding]) -> Vec<(&str, &str)> {
        findings
            .iter()
            .map(|f| (f.name.as_str(), f.subject.as_str()))
            .collect()
    }

    #[test]
    fn flags_unresolved_target() {
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node());
        fs.add_edge(Edge::new("index.md", "gone.md"));
        let composed = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        assert!(names(&findings).contains(&("unresolved-edge", "index.md")));
    }

    /// Compose an fs graph holding `files`, plus one edge carrying `raw`.
    fn graph_with_raw_edge(files: &[&str], source: &str, target: &str, raw: &str) -> Graph {
        let mut fs = Graph::labeled("fs");
        for f in files {
            fs.set_node(*f, fs_node());
        }
        let mut meta = Metadata::new();
        meta.insert("occurrences".into(), json!([{ "raw": raw }]));
        fs.add_edge(Edge::with_metadata(source, target, meta));
        compose(&GraphSet::new(vec![fs]))
    }

    #[test]
    fn wrong_base_cause_reaches_a_later_spelling() {
        // Two spellings resolve to the same missing target: an explicitly relative
        // one and a bare one that would resolve from the graph root. Because every
        // occurrence keeps its own `raw`, the cause finds the second — the first
        // is unambiguously relative by intent and names no cause on its own.
        //
        // The edge is built through `link_edges` rather than hand-rolled, so this
        // guards the aggregation as well as the rule: a first-occurrence-wins
        // builder would drop the bare spelling and the cause with it.
        let mut fs = Graph::labeled("fs");
        fs.set_node("docs/guide.md", fs_node());
        fs.set_node("config.md", fs_node());
        let links = [
            crate::parsers::Link {
                target: "./config.md".into(),
                line: Some(3),
            },
            crate::parsers::Link {
                target: "config.md".into(),
                line: Some(5),
            },
        ];
        let mut markdown = Graph::labeled("markdown");
        for edge in
            crate::builders::link_edges("docs/guide.md", &links, crate::builders::LinkPolicy::Body)
        {
            markdown.add_edge(edge);
        }
        let composed = compose(&GraphSet::new(vec![fs, markdown]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .expect("docs/config.md does not exist");
        assert_eq!(f.lines, vec![3, 5]);
        let cause = f.cause.as_deref().expect("expected a cause");
        assert!(
            cause.contains("`config.md` resolves from the graph root"),
            "got: {cause}"
        );
        assert!(cause.contains("../config.md"), "got: {cause}");
    }

    #[test]
    fn wrong_base_cause_names_the_cause() {
        // The #72 case: a repo-relative path in a doc one level down. The target
        // reported is a path nobody wrote, so the finding reads as a typo.
        let composed = graph_with_raw_edge(
            &["docs/taxonomy.md", "predicated/artifact/src/lib.rs"],
            "docs/taxonomy.md",
            "docs/predicated/artifact/src/lib.rs",
            "predicated/artifact/src/lib.rs",
        );
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let cause = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .and_then(|f| f.cause.as_deref())
            .expect("expected a cause");
        assert!(
            cause.contains("resolves from the graph root"),
            "got: {cause}"
        );
        assert!(
            cause.contains("../predicated/artifact/src/lib.rs"),
            "suggestion missing: {cause}"
        );
    }

    #[test]
    fn no_cause_when_root_path_also_missing() {
        // An ordinary typo: nothing resolves either way, so there is no cause to
        // name and the finding stands on its own.
        let composed = graph_with_raw_edge(
            &["docs/taxonomy.md"],
            "docs/taxonomy.md",
            "docs/typo.rs",
            "typo.rs",
        );
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert!(f.cause.is_none(), "got: {:?}", f.cause);
    }

    #[test]
    fn no_cause_for_explicitly_relative_paths() {
        // `./x.md` is relative by intent. A root `x.md` of the same name is a
        // coincidence, not a wrong base — naming a cause here would be noise.
        let composed = graph_with_raw_edge(
            &["docs/taxonomy.md", "x.md"],
            "docs/taxonomy.md",
            "docs/x.md",
            "./x.md",
        );
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert!(f.cause.is_none(), "got: {:?}", f.cause);
    }

    /// Compose a graph where `source` links `target` with the given fragments,
    /// and `target` defines `anchors`.
    fn graph_with_fragments(target_anchors: &[&str], occurrences: serde_json::Value) -> Graph {
        let mut fs = Graph::labeled("fs");
        fs.set_node("work-items.md", fs_node());
        fs.set_node("owners.md", fs_node());
        let mut markdown = Graph::labeled("markdown");
        markdown.set_node(
            "owners.md",
            Node::new(
                json!({ "anchors": target_anchors })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        );
        let mut meta = Metadata::new();
        meta.insert("occurrences".into(), occurrences);
        markdown.add_edge(Edge::with_metadata("work-items.md", "owners.md", meta));
        compose(&GraphSet::new(vec![fs, markdown]))
    }

    #[test]
    fn a_fragment_the_target_defines_is_quiet() {
        let composed = graph_with_fragments(
            &["security-console"],
            json!([{ "line": 3, "link": "owners.md#security-console" }]),
        );
        assert!(
            !names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-fragment")
        );
    }

    #[test]
    fn a_fragment_the_target_does_not_define_fires_on_its_own_line() {
        // The defect per-occurrence metadata was built for: one edge, two
        // anchors, only one of them wrong. The finding names the wrong one's line
        // and leaves the other alone.
        let composed = graph_with_fragments(
            &["security-console"],
            json!([
                { "line": 3, "link": "owners.md#security-console" },
                { "line": 7, "link": "owners.md#no-such-team" },
            ]),
        );
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-fragment")
            .expect("expected a finding");
        assert_eq!(f.target.as_deref(), Some("owners.md#no-such-team"));
        assert_eq!(f.lines, vec![7], "line 3 is fine and is not implicated");
        assert!(f.cause.is_none(), "nothing near it in case");
    }

    #[test]
    fn one_fragment_cited_from_several_lines_is_one_finding() {
        let composed = graph_with_fragments(
            &["security-console"],
            json!([
                { "line": 3, "link": "owners.md#typo" },
                { "line": 9, "link": "owners.md#typo" },
            ]),
        );
        let findings: Vec<_> = evaluate(&composed, &[crate::model::namespace("markdown")])
            .into_iter()
            .filter(|f| f.name == "unresolved-fragment")
            .collect();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].lines, vec![3, 9]);
    }

    #[test]
    fn a_case_mismatch_names_the_anchor_it_meant() {
        let composed = graph_with_fragments(
            &["ngwaf-edge"],
            json!([{ "line": 5, "link": "owners.md#NGWAF-Edge" }]),
        );
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-fragment")
            .expect("resolution is exact, so a case mismatch still fires");
        let cause = f.cause.as_deref().expect("expected a cause");
        assert!(cause.contains("`#ngwaf-edge`"), "got: {cause}");
    }

    #[test]
    fn a_percent_encoded_fragment_resolves() {
        // A browser percent-decodes before matching, and GitHub's own copied
        // permalink is encoded for a non-ASCII anchor.
        let composed = graph_with_fragments(
            &["café"],
            json!([{ "line": 3, "link": "owners.md#caf%C3%A9" }]),
        );
        assert!(
            !names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-fragment")
        );
    }

    #[test]
    fn decoding_does_not_accept_what_the_platform_rejects() {
        // `#OBS%2092` decodes to `OBS 92`, which is still not `obs-92`. Decoding
        // is not the loosening that slugging the citing side would be.
        let composed = graph_with_fragments(
            &["obs-92"],
            json!([{ "line": 3, "link": "owners.md#OBS%2092" }]),
        );
        assert!(
            names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-fragment")
        );
    }

    #[test]
    fn frontmatter_cannot_claim_anchors_a_file_does_not_have() {
        // Node metadata under `@frontmatter` is the author's own YAML. Only a
        // graph whose parser publishes anchors is authoritative about them.
        let mut fs = Graph::labeled("fs");
        fs.set_node("work-items.md", fs_node());
        fs.set_node("owners.md", fs_node());
        let mut frontmatter = Graph::labeled("frontmatter");
        frontmatter.set_node(
            "owners.md",
            Node::new(
                json!({ "anchors": ["invented"] })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        );
        let mut markdown = Graph::labeled("markdown");
        let mut meta = Metadata::new();
        meta.insert(
            "occurrences".into(),
            json!([{ "line": 3, "link": "owners.md#invented" }]),
        );
        markdown.add_edge(Edge::with_metadata("work-items.md", "owners.md", meta));
        let composed = compose(&GraphSet::new(vec![fs, frontmatter, markdown]));

        // `owners.md` was never read as markdown, so its fragments stay unknown
        // however its frontmatter is written — no finding, and no false clearance.
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        assert!(
            !names(&findings)
                .iter()
                .any(|(n, _)| *n == "unresolved-fragment")
        );
    }

    #[test]
    fn a_bare_hash_is_the_top_of_the_page() {
        let composed = graph_with_fragments(&["a"], json!([{ "line": 2, "link": "owners.md#" }]));
        assert!(
            !names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-fragment")
        );
    }

    #[test]
    fn a_target_no_parser_read_has_unknown_fragments() {
        // No graph published an `anchors` list for `src/lib.rs`, so a fragment
        // into it is unknown rather than broken.
        let mut fs = Graph::labeled("fs");
        fs.set_node("guide.md", fs_node());
        fs.set_node("src/lib.rs", fs_node());
        let mut markdown = Graph::labeled("markdown");
        let mut meta = Metadata::new();
        meta.insert(
            "occurrences".into(),
            json!([{ "line": 4, "link": "src/lib.rs#L20" }]),
        );
        markdown.add_edge(Edge::with_metadata("guide.md", "src/lib.rs", meta));
        let composed = compose(&GraphSet::new(vec![fs, markdown]));
        assert!(
            !names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-fragment")
        );
    }

    #[test]
    fn a_read_target_defining_nothing_makes_every_fragment_broken() {
        // The other half of the `None`/empty distinction: a parser read it and it
        // defines no addresses, so the fragment is broken rather than unknown.
        let composed =
            graph_with_fragments(&[], json!([{ "line": 4, "link": "owners.md#anything" }]));
        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        assert!(
            findings.iter().any(|f| f.name == "unresolved-fragment"),
            "got {:?}",
            names(&findings)
        );
    }

    #[test]
    fn an_unresolvable_target_reports_only_the_edge() {
        // The fragment is the lesser half of the same mistake; reporting both
        // would double-count one broken link.
        let mut fs = Graph::labeled("fs");
        fs.set_node("guide.md", fs_node());
        let mut markdown = Graph::labeled("markdown");
        let mut meta = Metadata::new();
        meta.insert(
            "occurrences".into(),
            json!([{ "line": 4, "link": "gone.md#section" }]),
        );
        markdown.add_edge(Edge::with_metadata("guide.md", "gone.md", meta));
        let composed = compose(&GraphSet::new(vec![fs, markdown]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let found = names(&findings);
        assert!(
            found.contains(&("unresolved-edge", "guide.md")),
            "got {found:?}"
        );
        assert!(!found.iter().any(|(name, _)| *name == "unresolved-fragment"));
    }

    #[test]
    fn unresolved_edge_carries_link_lines() {
        // A markdown link to a missing target on line 3 — the finding points there.
        let mut markdown = Graph::labeled("markdown");
        let mut meta = Metadata::new();
        meta.insert("occurrences".into(), json!([{ "line": 3 }]));
        markdown.add_edge(Edge::with_metadata("index.md", "gone.md", meta));
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node());
        let composed = compose(&GraphSet::new(vec![fs, markdown]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert_eq!(f.lines, vec![3]);
        assert!(
            f.format_text().contains("index.md:3 → gone.md"),
            "got: {}",
            f.format_text()
        );
    }

    #[test]
    fn does_not_flag_uri_target() {
        let mut markdown = Graph::labeled("markdown");
        markdown.add_edge(Edge::new("index.md", "https://example.com"));
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node());
        let composed = compose(&GraphSet::new(vec![fs, markdown]));

        assert!(
            !names(&evaluate(&composed, &[crate::model::namespace("markdown")]))
                .iter()
                .any(|(name, _)| *name == "unresolved-edge")
        );
    }

    #[test]
    fn flags_detached_node() {
        let mut fs = Graph::labeled("fs");
        fs.set_node("lonely.md", fs_node());
        fs.set_node("a.md", fs_node());
        fs.set_node("b.md", fs_node());
        fs.add_edge(Edge::new("a.md", "b.md"));
        let composed = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let n = names(&findings);
        assert!(n.contains(&("detached-node", "lonely.md")), "got {n:?}");
        assert!(!n.contains(&("detached-node", "a.md")));
        assert!(!n.contains(&("detached-node", "b.md")));
    }

    #[test]
    fn directory_node_is_not_detached() {
        // A link-less directory is scaffolding, not orphaned content.
        let mut fs = Graph::labeled("fs");
        fs.set_node(
            "guides",
            Node::new(json!({ "type": "directory" }).as_object().unwrap().clone()),
        );
        fs.set_node("lonely.md", fs_node());
        let composed = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&composed, &[crate::model::namespace("markdown")]);
        let n = names(&findings);
        assert!(
            !n.contains(&("detached-node", "guides")),
            "directory should not be flagged detached, got {n:?}"
        );
        // A genuinely orphaned file is still flagged.
        assert!(n.contains(&("detached-node", "lonely.md")));
    }

    /// The cause is data on the finding, and it is serialised. Escaping it here —
    /// where it is produced rather than where it is rendered — put a literal
    /// backslash-n into JSON, against the guarantee that JSON carries what the
    /// file carried. The hand-built `Finding` test in `diagnostic` cannot see
    /// this: it never runs this producer.
    #[test]
    fn the_cause_carries_the_literal_bytes_not_their_rendering() {
        let mut fs = Graph::labeled("fs");
        fs.set_node("docs/guide.md", fs_node());
        fs.set_node("we\nird.md", fs_node());
        let links = [crate::parsers::Link {
            target: "we\nird.md".into(),
            line: Some(2),
        }];
        let mut markdown = Graph::labeled("markdown");
        for edge in
            crate::builders::link_edges("docs/guide.md", &links, crate::builders::LinkPolicy::Body)
        {
            markdown.add_edge(edge);
        }
        let composed = compose(&GraphSet {
            graphs: vec![fs, markdown],
        });
        let edge = composed
            .edges
            .iter()
            .find(|e| e.source == "docs/guide.md")
            .expect("one edge");

        let cause =
            wrong_base_cause(&composed, edge).expect("the bare spelling resolves from the root");
        assert!(
            cause.contains('\n'),
            "the producer must keep the bytes: {cause:?}"
        );
        assert!(
            !cause.contains("\\n"),
            "escaping belongs to the rendering: {cause:?}"
        );
    }
}
