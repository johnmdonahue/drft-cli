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
            findings.push(
                Finding::warn(
                    "unresolved-edge",
                    &edge.source,
                    edge_provenance(edge),
                    "no defining node",
                )
                .with_target(&edge.target)
                .with_lines(edge.lines()),
            );
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

    #[test]
    fn unresolved_edge_carries_link_lines() {
        // A markdown link to a missing target on line 3 — the finding points there.
        let mut markdown = Graph::labeled("markdown");
        let mut meta = Metadata::new();
        meta.insert("lines".into(), json!([3]));
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
