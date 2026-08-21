//! Composition: the projection from the raw set of per-graph fragments into a
//! single composed graph. This is the only module that knows about more than one
//! graph, and the only place the reserved `@` and `_` sigils appear.
//!
//! Composition merges by **path coincidence**:
//!
//! - **Nodes** are keyed by path. Each contributing graph's bare metadata nests
//!   under its `@<graph>` namespace; the reserved `_graphs` key lists every
//!   contributing namespace.
//! - **Edges** are deduped by `(source, target)`. Per-graph edge metadata, when
//!   present, nests under `@<graph>`; `_graphs` lists contributors.
//! - **Resolution** is namespace presence: a path that appears only as an edge
//!   target — with no `@fs` block — is an unresolved/external reference and gets
//!   no node entry.
//!
//! v0.8's graphs are all colocated, so no contribution edges are auto-emitted
//! here; that mechanism arrives with the first separated graph.

use serde_json::Value;

use crate::model::{Edge, Graph, GraphSet, Metadata, PROVENANCE_KEY, namespace};

/// Merge a raw set of per-graph fragments into one composed graph.
pub fn compose(set: &GraphSet) -> Graph {
    let mut composed = Graph::composed();

    for fragment in &set.graphs {
        let namespace = namespace_of(fragment);

        for (path, node) in &fragment.nodes {
            let entry = composed.nodes.entry(path.clone()).or_default();
            entry
                .metadata
                .insert(namespace.clone(), Value::Object(node.metadata.clone()));
            add_provenance(&mut entry.metadata, &namespace);
        }
    }

    // Edges merge by (source, target); track insertion order for determinism via
    // the order graphs and their edges appear, then sort at the end.
    let mut edge_index: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();

    for fragment in &set.graphs {
        let namespace = namespace_of(fragment);

        for edge in &fragment.edges {
            let key = (edge.source.clone(), edge.target.clone());
            let idx = *edge_index.entry(key).or_insert_with(|| {
                composed.edges.push(Edge::with_metadata(
                    &edge.source,
                    &edge.target,
                    Metadata::new(),
                ));
                composed.edges.len() - 1
            });
            let meta = &mut composed.edges[idx].metadata;
            if !edge.metadata.is_empty() {
                meta.insert(namespace.clone(), Value::Object(edge.metadata.clone()));
            }
            add_provenance(meta, &namespace);
        }
    }

    composed.sort_edges();

    composed
}

/// The `@<label>` namespace key for a fragment. Fragments always carry a label;
/// fall back to `@graph` defensively if one is missing.
fn namespace_of(fragment: &Graph) -> String {
    match &fragment.label {
        Some(label) => namespace(label),
        None => "@graph".to_string(),
    }
}

/// Append `namespace` to the `_graphs` provenance list, keeping it sorted and
/// unique.
fn add_provenance(metadata: &mut Metadata, namespace: &str) {
    let list = metadata
        .entry(PROVENANCE_KEY)
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(entries) = list {
        let value = Value::String(namespace.to_string());
        if !entries.contains(&value) {
            entries.push(value);
            entries.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use serde_json::json;

    fn obj(value: Value) -> Metadata {
        value.as_object().unwrap().clone()
    }

    fn fragment_with_node(label: &str, path: &str, meta: Value) -> Graph {
        let mut g = Graph::labeled(label);
        g.set_node(path, Node::new(obj(meta)));
        g
    }

    #[test]
    fn single_graph_namespaces_metadata() {
        let fs = fragment_with_node("fs", "a.md", json!({ "type": "file", "hash": "b3:1" }));
        let composed = compose(&GraphSet::new(vec![fs]));

        let node = &composed.nodes["a.md"];
        assert_eq!(
            node.metadata["@fs"],
            json!({ "type": "file", "hash": "b3:1" })
        );
        assert_eq!(node.metadata[PROVENANCE_KEY], json!(["@fs"]));
    }

    #[test]
    fn multiple_graphs_merge_by_path() {
        let fs = fragment_with_node("fs", "a.md", json!({ "type": "file", "hash": "b3:1" }));
        let frontmatter = fragment_with_node(
            "frontmatter",
            "a.md",
            json!({ "title": "A", "status": "draft" }),
        );

        let composed = compose(&GraphSet::new(vec![fs, frontmatter]));
        let node = &composed.nodes["a.md"];
        assert_eq!(node.metadata["@fs"]["type"], json!("file"));
        assert_eq!(node.metadata["@frontmatter"]["title"], json!("A"));
        assert_eq!(
            node.metadata[PROVENANCE_KEY],
            json!(["@frontmatter", "@fs"])
        );
    }

    #[test]
    fn edges_dedup_and_collect_provenance() {
        let mut markdown = Graph::labeled("markdown");
        markdown.add_edge(Edge::new("a.md", "b.md"));
        let mut frontmatter = Graph::labeled("frontmatter");
        frontmatter.add_edge(Edge::new("a.md", "b.md"));

        let composed = compose(&GraphSet::new(vec![markdown, frontmatter]));
        assert_eq!(composed.edges.len(), 1);
        assert_eq!(
            composed.edges[0].metadata[PROVENANCE_KEY],
            json!(["@frontmatter", "@markdown"])
        );
    }

    #[test]
    fn edge_metadata_nests_under_namespace() {
        let mut markdown = Graph::labeled("markdown");
        markdown.add_edge(Edge::with_metadata(
            "a.md",
            "b.md",
            obj(json!({ "occurrences": [{ "line": 4, "link": "b.md#x" }] })),
        ));
        let composed = compose(&GraphSet::new(vec![markdown]));
        assert_eq!(
            composed.edges[0].metadata["@markdown"]["occurrences"],
            json!([{ "line": 4, "link": "b.md#x" }])
        );
    }
}
