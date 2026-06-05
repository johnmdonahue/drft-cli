//! Core graph data model and JGF (JSON Graph Format) serialization.
//!
//! drft's substrate is a *set of independent graphs* (the raw view); a
//! composition step merges them by path into one graph (the composed view).
//! Both views share the same node/edge shape and both serialize to valid JGF:
//!
//! - **Composed** — a single graph: `{"graph": {...}}` ([`GraphDocument`]).
//! - **Raw** — the unmerged set: `{"graphs": [...]}` ([`GraphSet`]), JGF's
//!   multi-graph form.
//!
//! A node or edge carries a JSON object of [`Metadata`]. The keys differ by
//! view:
//!
//! - In a **raw** per-graph fragment, keys are *bare* — whatever the builder
//!   emits (e.g. `{"type": "file", "hash": "b3:…"}`).
//! - In the **composed** graph, keys are *namespaced*: an `@<graph>` object per
//!   contributing graph, plus the reserved `_graphs` provenance list.
//!
//! `@` and `_` are reserved, compose-only sigils. A graph label must not contain
//! `@` (it builds the `@<label>` namespace) or start with `_` (reserved for keys
//! like `_graphs`); interior `_` is fine. [`validate_label`],
//! [`validate_raw_metadata`], and [`validate_composed_metadata`] enforce these
//! invariants.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The reserved provenance key stamped on composed nodes and edges.
pub const PROVENANCE_KEY: &str = "_graphs";

/// The `fs` namespace key on a composed node — the base graph that carries
/// content type and hash. The single place the `@fs` literal lives.
pub const FS_NAMESPACE: &str = "@fs";

/// The `@<label>` namespace key under which a graph's contribution nests in a
/// composed node or edge. The single place the `@` prefix rule lives;
/// [`FS_NAMESPACE`] is `namespace("fs")`.
pub fn namespace(label: &str) -> String {
    format!("@{label}")
}

/// A JSON object of metadata attached to a node or edge.
///
/// `serde_json::Map` (with the default feature set) is backed by a `BTreeMap`,
/// so key order is sorted and deterministic — important for golden tests and
/// reproducible output.
pub type Metadata = Map<String, Value>;

/// A node in a graph. Its identity is its key in [`Graph::nodes`] (a path);
/// the node body carries only metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Node {
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Metadata,
}

impl Node {
    /// A node with the given metadata object.
    pub fn new(metadata: Metadata) -> Self {
        Self { metadata }
    }

    /// This composed node's current `fs` content hash, if it has one.
    pub fn fs_hash(&self) -> Option<&str> {
        self.metadata.get(FS_NAMESPACE)?.get("hash")?.as_str()
    }

    /// Whether this composed node is resolved — present with an `@fs` block.
    /// Resolution is namespace presence.
    pub fn is_resolved(&self) -> bool {
        self.metadata.contains_key(FS_NAMESPACE)
    }
}

/// A directed edge from `source` to `target` (both node-identity paths).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Metadata,
}

impl Edge {
    /// An edge from `source` to `target` with no metadata.
    pub fn new(source: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            metadata: Metadata::new(),
        }
    }

    /// An edge from `source` to `target` carrying the given metadata.
    pub fn with_metadata(
        source: impl Into<String>,
        target: impl Into<String>,
        metadata: Metadata,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            metadata,
        }
    }
}

/// A single JGF graph. Used for both a raw per-graph fragment (with `label`
/// set to the graph name) and the composed graph (with `label` absent).
///
/// Nodes are keyed by path in a `BTreeMap` for deterministic, sorted output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    /// The graph name, present in a raw fragment, absent in the composed graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub directed: bool,
    #[serde(default)]
    pub nodes: BTreeMap<String, Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl Graph {
    /// An empty composed (unlabeled) directed graph.
    pub fn composed() -> Self {
        Self {
            label: None,
            directed: true,
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    /// An empty labeled directed graph (a raw per-graph fragment).
    pub fn labeled(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            directed: true,
            nodes: BTreeMap::new(),
            edges: Vec::new(),
        }
    }

    /// Insert or replace the node at `path`.
    pub fn set_node(&mut self, path: impl Into<String>, node: Node) {
        self.nodes.insert(path.into(), node);
    }

    /// Append an edge.
    pub fn add_edge(&mut self, edge: Edge) {
        self.edges.push(edge);
    }

    /// Sort edges by `(source, target)` for deterministic output.
    pub fn sort_edges(&mut self) {
        self.edges.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
        });
    }

    /// Wrap this graph as a composed JGF document (`{"graph": {...}}`).
    pub fn into_document(self) -> GraphDocument {
        GraphDocument { graph: self }
    }
}

/// JGF single-graph document: `{"graph": {...}}`. The composed view drft emits
/// by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphDocument {
    pub graph: Graph,
}

/// JGF multi-graph document: `{"graphs": [...]}`. The raw view drft emits under
/// `--raw` — the unmerged set of per-graph fragments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphSet {
    pub graphs: Vec<Graph>,
}

impl GraphSet {
    /// A set from the given graphs.
    pub fn new(graphs: Vec<Graph>) -> Self {
        Self { graphs }
    }
}

/// An invariant violation in graph labels or metadata keys.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("graph label must not be empty")]
    EmptyLabel,
    #[error("graph label '{0}' must not contain '@' or start with '_'")]
    SigilInLabel(String),
    #[error("raw metadata key '{0}' must be bare (no leading '@' or '_')")]
    SigilInRawKey(String),
    #[error(
        "composed metadata key '{0}' is invalid: expected an '@<graph>' namespace or '_graphs'"
    )]
    InvalidComposedKey(String),
    #[error("composed metadata namespace '@{0}' must name a bare graph (no '@' or '_')")]
    InvalidNamespace(String),
}

/// Validate that a graph label is non-empty and free of the reserved sigils: no
/// `@` anywhere (it builds the `@<label>` namespace) and no leading `_`
/// (reserved for keys like `_graphs`). Interior `_` is allowed.
pub fn validate_label(label: &str) -> Result<(), ValidationError> {
    if label.is_empty() {
        return Err(ValidationError::EmptyLabel);
    }
    if label.contains('@') || label.starts_with('_') {
        return Err(ValidationError::SigilInLabel(label.to_string()));
    }
    Ok(())
}

/// Validate that a raw fragment's metadata keys are all bare — no key may begin
/// with the reserved `@` or `_` sigils. Builders emit bare keys; the sigils are
/// introduced only at compose.
pub fn validate_raw_metadata(metadata: &Metadata) -> Result<(), ValidationError> {
    for key in metadata.keys() {
        if key.starts_with('@') || key.starts_with('_') {
            return Err(ValidationError::SigilInRawKey(key.clone()));
        }
    }
    Ok(())
}

/// Validate that a composed metadata object's top-level keys are each either an
/// `@<graph>` namespace (naming a bare graph) or the reserved `_graphs` key.
pub fn validate_composed_metadata(metadata: &Metadata) -> Result<(), ValidationError> {
    for key in metadata.keys() {
        if key == PROVENANCE_KEY {
            continue;
        }
        match key.strip_prefix('@') {
            Some(name) => validate_label(name)
                .map_err(|_| ValidationError::InvalidNamespace(name.to_string()))?,
            None => return Err(ValidationError::InvalidComposedKey(key.clone())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(value: Value) -> Metadata {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn composed_document_round_trips() {
        let mut graph = Graph::composed();
        graph.set_node(
            "src/graph.rs",
            Node::new(meta(json!({
                "@fs": { "type": "file", "hash": "b3:444" },
                "_graphs": ["@fs"]
            }))),
        );
        graph.set_node(
            "docs/architecture.md",
            Node::new(meta(json!({
                "@fs": { "type": "file", "hash": "b3:222" },
                "@frontmatter": { "title": "Architecture", "status": "draft" },
                "_graphs": ["@fs", "@frontmatter"]
            }))),
        );
        graph.add_edge(Edge::with_metadata(
            "docs/architecture.md",
            "src/graph.rs",
            meta(json!({ "_graphs": ["@markdown", "@frontmatter"] })),
        ));

        let doc = graph.into_document();
        let json = serde_json::to_value(&doc).unwrap();

        // Composed envelope: top-level "graph", no "label" inside.
        assert!(json.get("graph").is_some());
        assert!(json["graph"].get("label").is_none());
        assert_eq!(json["graph"]["directed"], json!(true));

        let back: GraphDocument = serde_json::from_value(json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn raw_set_round_trips() {
        let mut fs = Graph::labeled("fs");
        fs.set_node(
            "src/graph.rs",
            Node::new(meta(json!({ "type": "file", "hash": "b3:444" }))),
        );

        let mut markdown = Graph::labeled("markdown");
        markdown.add_edge(Edge::new("docs/architecture.md", "src/graph.rs"));

        let set = GraphSet::new(vec![fs, markdown]);
        let json = serde_json::to_value(&set).unwrap();

        // Raw envelope: top-level "graphs" array, each fragment labeled.
        let graphs = json["graphs"].as_array().unwrap();
        assert_eq!(graphs.len(), 2);
        assert_eq!(graphs[0]["label"], json!("fs"));
        assert_eq!(graphs[1]["label"], json!("markdown"));

        let back: GraphSet = serde_json::from_value(json).unwrap();
        assert_eq!(set, back);
    }

    #[test]
    fn empty_metadata_is_omitted() {
        let mut graph = Graph::composed();
        graph.set_node("a.md", Node::default());
        graph.add_edge(Edge::new("a.md", "b.md"));
        let json = serde_json::to_value(graph.into_document()).unwrap();
        assert!(json["graph"]["nodes"]["a.md"].get("metadata").is_none());
        assert!(json["graph"]["edges"][0].get("metadata").is_none());
    }

    #[test]
    fn node_keys_are_sorted() {
        let mut graph = Graph::composed();
        graph.set_node("z.md", Node::default());
        graph.set_node("a.md", Node::default());
        graph.set_node("m.md", Node::default());
        let json = serde_json::to_string(&graph.into_document()).unwrap();
        let a = json.find("a.md").unwrap();
        let m = json.find("m.md").unwrap();
        let z = json.find("z.md").unwrap();
        assert!(a < m && m < z, "node keys should serialize in sorted order");
    }

    #[test]
    fn validate_label_accepts_bare() {
        assert!(validate_label("fs").is_ok());
        assert!(validate_label("markdown").is_ok());
        assert!(validate_label("frontmatter").is_ok());
    }

    #[test]
    fn validate_label_rejects_sigils_and_empty() {
        assert_eq!(validate_label(""), Err(ValidationError::EmptyLabel));
        assert!(matches!(
            validate_label("@fs"),
            Err(ValidationError::SigilInLabel(_))
        ));
        assert!(matches!(
            validate_label("_internal"),
            Err(ValidationError::SigilInLabel(_))
        ));
        // Interior underscore is allowed.
        assert!(validate_label("design_docs").is_ok());
    }

    #[test]
    fn validate_raw_metadata_rejects_sigil_keys() {
        assert!(validate_raw_metadata(&meta(json!({ "type": "file" }))).is_ok());
        assert!(matches!(
            validate_raw_metadata(&meta(json!({ "@fs": {} }))),
            Err(ValidationError::SigilInRawKey(_))
        ));
        assert!(matches!(
            validate_raw_metadata(&meta(json!({ "_graphs": [] }))),
            Err(ValidationError::SigilInRawKey(_))
        ));
    }

    #[test]
    fn validate_composed_metadata_accepts_namespaces_and_provenance() {
        assert!(
            validate_composed_metadata(&meta(json!({
                "@fs": { "type": "file" },
                "_graphs": ["@fs"]
            })))
            .is_ok()
        );
    }

    #[test]
    fn validate_composed_metadata_rejects_bare_and_bad_namespace() {
        assert!(matches!(
            validate_composed_metadata(&meta(json!({ "type": "file" }))),
            Err(ValidationError::InvalidComposedKey(_))
        ));
        assert!(matches!(
            validate_composed_metadata(&meta(json!({ "@_internal": {} }))),
            Err(ValidationError::InvalidNamespace(_))
        ));
    }
}
