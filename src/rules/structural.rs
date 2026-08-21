//! Structural rules: findings derived from graph shape alone, no lockfile.
//!
//! Findings: `unresolved-edge` (an edge target with no `@fs` block — no defining
//! node) and `detached-node` (a node with no inbound or outbound edges). URI
//! targets are intentional external references, not unresolved.

use std::collections::HashSet;

use crate::diagnostic::Finding;
use crate::model::{Graph, Node};
use crate::rules::{edge_provenance, provenance};
use crate::util::is_uri;

/// Evaluate structural findings for `graph`.
pub fn evaluate(graph: &Graph) -> Vec<Finding> {
    let mut findings = Vec::new();

    // unresolved-edge: a non-URI edge target with no defining node.
    for edge in &graph.edges {
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

        let findings = evaluate(&composed);
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
        let mut fs = Graph::labeled("fs");
        fs.set_node("docs/guide.md", fs_node());
        fs.set_node("config.md", fs_node());
        let mut meta = Metadata::new();
        meta.insert(
            "occurrences".into(),
            json!([
                { "line": 3, "raw": "./config.md" },
                { "line": 5, "raw": "config.md" },
            ]),
        );
        fs.add_edge(Edge::with_metadata("docs/guide.md", "docs/config.md", meta));
        let composed = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&composed);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .expect("target does not exist");
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
        let findings = evaluate(&composed);
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
        let findings = evaluate(&composed);
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
        let findings = evaluate(&composed);
        let f = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert!(f.cause.is_none(), "got: {:?}", f.cause);
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

        let findings = evaluate(&composed);
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
            !names(&evaluate(&composed))
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

        let findings = evaluate(&composed);
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

        let findings = evaluate(&composed);
        let n = names(&findings);
        assert!(
            !n.contains(&("detached-node", "guides")),
            "directory should not be flagged detached, got {n:?}"
        );
        // A genuinely orphaned file is still flagged.
        assert!(n.contains(&("detached-node", "lonely.md")));
    }
}
