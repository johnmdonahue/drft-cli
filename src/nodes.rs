//! `drft nodes`: project node metadata for a set of paths — the read verb that
//! grounds an LLM (or a human) on what the graph knows about each node.
//!
//! `nodes` is a *reader*: its selectors expand (an exact path, a subtree, or a
//! glob), which is safe because a read has no side effect. This module owns the
//! pure projection and rendering — narrowing each node's composed metadata by
//! namespace (which lens) and field (which keys), then formatting it as compact
//! text or JSON. Selector-to-key resolution and namespace validation live in
//! `main.rs`, next to the shared `node_candidates`/`resolve_node` resolver and
//! the loaded config they need.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::model::{Graph, namespace};

/// Normalize a `--namespace` value to its `@`-prefixed metadata key. Accepts the
/// bare config name (`frontmatter`) or the already-prefixed form (`@frontmatter`),
/// since the prefixed form appears in JSON output and gets copied from there.
/// Validation against the declared graphs happens at the call site, where the
/// config is in hand.
pub fn normalize_namespace(name: &str) -> String {
    namespace(name.strip_prefix('@').unwrap_or(name))
}

/// A projected node: its id and the namespaced metadata that survived the
/// namespace/field filters. `metadata` holds only `@<graph>` blocks — the
/// `_graphs` provenance is dropped, since this is a focused projection for
/// grounding, not the full-fidelity composed node (`drft graph` gives that).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NodeProjection {
    pub id: String,
    pub metadata: Map<String, Value>,
}

/// Project `keys` out of `graph`, narrowing each node's metadata to `namespaces`
/// (empty = every `@` namespace) and `fields` (empty = every key).
///
/// A node drops out when a namespace filter is set and it carries none of the
/// requested namespaces, or when a field filter is set and none of the requested
/// fields appear in any shown namespace. Both are legitimate empties (exit zero),
/// not errors: a field filter answers *which* nodes declare the field.
pub fn project(
    graph: &Graph,
    keys: &[String],
    namespaces: &[String],
    fields: &[String],
) -> Vec<NodeProjection> {
    let mut out = Vec::new();
    for key in keys {
        let Some(node) = graph.nodes.get(key) else {
            continue;
        };
        let mut metadata = Map::new();
        for (ns, value) in &node.metadata {
            // Only `@<graph>` lenses are projected; `_graphs` provenance is not.
            if !ns.starts_with('@') {
                continue;
            }
            if !namespaces.is_empty() && !namespaces.contains(ns) {
                continue;
            }
            let block = if fields.is_empty() {
                value.clone()
            } else {
                let Some(obj) = value.as_object() else {
                    continue;
                };
                let filtered: Map<String, Value> = obj
                    .iter()
                    .filter(|(k, _)| fields.iter().any(|f| f == *k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if filtered.is_empty() {
                    continue;
                }
                Value::Object(filtered)
            };
            metadata.insert(ns.clone(), block);
        }
        // A namespace or field filter that left nothing means this node does not
        // carry what was asked for — drop it rather than emit an empty block.
        if metadata.is_empty() && (!namespaces.is_empty() || !fields.is_empty()) {
            continue;
        }
        out.push(NodeProjection {
            id: key.clone(),
            metadata,
        });
    }
    out
}

/// Render projected nodes as a compact, LLM-legible block per node: the node id
/// flush-left, each namespace indented under it, each field under that. Blocks
/// are separated by a blank line and the output ends in a newline. Scalars render
/// inline; arrays and objects render as compact JSON. An empty projection renders
/// to the empty string (no trailing newline), so piping stays clean.
pub fn format_text(nodes: &[NodeProjection]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    let mut blocks = Vec::new();
    for node in nodes {
        let mut lines = vec![node.id.clone()];
        for (ns, value) in &node.metadata {
            lines.push(format!("  {ns}"));
            match value.as_object() {
                Some(obj) => {
                    for (k, v) in obj {
                        lines.push(format!("    {k}: {}", render_value(v)));
                    }
                }
                None => lines.push(format!("    {}", render_value(value))),
            }
        }
        blocks.push(lines.join("\n"));
    }
    format!("{}\n", blocks.join("\n\n"))
}

/// Render a metadata value for text output. An ordinary string prints bare; a
/// string carrying control characters — a YAML block scalar's newlines, say —
/// would break the one-line-per-field layout (and an embedded blank line could
/// read as a node separator), so it renders as compact JSON like every array,
/// object, number, or bool: the controls are escaped and the field stays one line.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(s) if !s.chars().any(char::is_control) => s.clone(),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn sample_graph() -> Graph {
        let mut g = Graph::composed();
        g.set_node(
            "docs/a.md",
            Node::new(obj(json!({
                "@fs": { "type": "file", "hash": "b3:1" },
                "@frontmatter": { "purpose": "explain a", "status": "draft" },
                "_graphs": ["@frontmatter", "@fs"]
            }))),
        );
        g.set_node(
            "docs/b.md",
            Node::new(obj(json!({
                "@fs": { "type": "file", "hash": "b3:2" },
                "@frontmatter": { "status": "final" },
                "_graphs": ["@frontmatter", "@fs"]
            }))),
        );
        g.set_node(
            "src/lib.rs",
            Node::new(obj(json!({
                "@fs": { "type": "file", "hash": "b3:3" },
                "_graphs": ["@fs"]
            }))),
        );
        g
    }

    #[test]
    fn normalize_namespace_accepts_bare_and_prefixed() {
        assert_eq!(normalize_namespace("frontmatter"), "@frontmatter");
        assert_eq!(normalize_namespace("@fs"), "@fs");
    }

    #[test]
    fn project_without_filters_keeps_all_namespaces() {
        let g = sample_graph();
        let out = project(&g, &["docs/a.md".into()], &[], &[]);
        assert_eq!(out.len(), 1);
        // Provenance is dropped; every `@` lens is kept.
        assert!(out[0].metadata.contains_key("@fs"));
        assert!(out[0].metadata.contains_key("@frontmatter"));
        assert!(!out[0].metadata.contains_key("_graphs"));
    }

    #[test]
    fn namespace_filter_drops_nodes_without_the_lens() {
        let g = sample_graph();
        let keys = vec!["docs/a.md".into(), "src/lib.rs".into()];
        let out = project(&g, &keys, &["@frontmatter".into()], &[]);
        // src/lib.rs has no @frontmatter block, so it drops out.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "docs/a.md");
        assert_eq!(
            out[0].metadata.keys().collect::<Vec<_>>(),
            vec!["@frontmatter"]
        );
    }

    #[test]
    fn field_filter_lists_only_nodes_that_declare_it() {
        let g = sample_graph();
        let keys = vec!["docs/a.md".into(), "docs/b.md".into()];
        let out = project(&g, &keys, &["@frontmatter".into()], &["purpose".into()]);
        // Only a.md declares `purpose`; b.md drops out.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "docs/a.md");
        assert_eq!(
            out[0].metadata["@frontmatter"],
            json!({ "purpose": "explain a" })
        );
    }

    #[test]
    fn field_filter_with_no_matches_is_empty_not_error() {
        let g = sample_graph();
        let keys = vec!["docs/a.md".into(), "docs/b.md".into()];
        let out = project(&g, &keys, &["@frontmatter".into()], &["nonexistent".into()]);
        assert!(out.is_empty());
    }

    #[test]
    fn format_text_is_one_block_per_node() {
        let g = sample_graph();
        let out = project(&g, &["docs/a.md".into()], &["@frontmatter".into()], &[]);
        let text = format_text(&out);
        assert_eq!(
            text,
            "docs/a.md\n  @frontmatter\n    purpose: explain a\n    status: draft\n"
        );
    }

    #[test]
    fn format_text_empty_is_blank() {
        assert_eq!(format_text(&[]), "");
    }

    #[test]
    fn multiline_string_renders_escaped_to_stay_one_line() {
        // A YAML block scalar's newline must not break the one-line-per-field layout
        // or read as a node separator: it renders as escaped JSON on a single line.
        let mut g = Graph::composed();
        g.set_node(
            "docs/a.md",
            Node::new(obj(json!({
                "@frontmatter": { "purpose": "line one\nline two" },
                "@fs": { "type": "file", "hash": "b3:1" }
            }))),
        );
        let out = project(
            &g,
            &["docs/a.md".into()],
            &["@frontmatter".into()],
            &["purpose".into()],
        );
        assert_eq!(
            format_text(&out),
            "docs/a.md\n  @frontmatter\n    purpose: \"line one\\nline two\"\n"
        );
    }
}
