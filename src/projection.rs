//! Shared projection helpers for the reader verbs (`nodes`, `edges`). The verbs
//! differ in what they project — a node's metadata by id, an edge's by
//! `source → target` — but share the metadata grammar (`--namespace`/`--field`)
//! and the compact, LLM-legible text shape. This module owns those shared pieces
//! so the two verbs stay provably consistent.

use serde_json::{Map, Value};

/// Narrow a composed metadata object to `namespaces` (empty = every `@` block) and
/// `fields` (empty = every key within a block). Keeps only `@<graph>` lenses — the
/// `_graphs` provenance is dropped. An empty return means the object carries none
/// of what was asked for; the caller decides whether that drops the node or edge.
pub fn filter_metadata(
    metadata: &Map<String, Value>,
    namespaces: &[String],
    fields: &[String],
) -> Map<String, Value> {
    let mut out = Map::new();
    for (ns, value) in metadata {
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
        out.insert(ns.clone(), block);
    }
    out
}

/// Whether a projected entry drops out under the given filters. An entry whose
/// filtered metadata came back empty carries none of the requested
/// namespaces/fields, so a filter that asked for some means "not this one". With no
/// filter, an empty metadata object is kept — a node always has `@fs`, and an edge
/// with no per-graph metadata is still an edge.
pub fn filtered_out(
    metadata: &Map<String, Value>,
    namespaces: &[String],
    fields: &[String],
) -> bool {
    metadata.is_empty() && (!namespaces.is_empty() || !fields.is_empty())
}

/// Append a projected metadata object to `lines` as indented text: `  @namespace`
/// then `    key: value` per field. Sits under a header line — a node id, or an
/// edge's `source → target`.
pub fn push_metadata_lines(lines: &mut Vec<String>, metadata: &Map<String, Value>) {
    for (ns, value) in metadata {
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
}

/// Join per-entry blocks into the final text: one blank line between blocks and a
/// trailing newline, or the empty string when there is nothing to show (so piping
/// stays clean).
pub fn join_blocks(blocks: Vec<String>) -> String {
    if blocks.is_empty() {
        return String::new();
    }
    format!("{}\n", blocks.join("\n\n"))
}

/// Render a metadata value for text output. An ordinary string prints bare; a
/// string carrying control characters — a YAML block scalar's newlines, say —
/// would break the one-line-per-field layout (and an embedded blank line could
/// read as a block separator), so it renders as compact JSON like every array,
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
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn filter_drops_provenance_and_keeps_all_by_default() {
        let m = obj(json!({ "@markdown": { "lines": [4] }, "_graphs": ["@markdown"] }));
        let out = filter_metadata(&m, &[], &[]);
        assert_eq!(out.keys().collect::<Vec<_>>(), vec!["@markdown"]);
    }

    #[test]
    fn filter_by_namespace_and_field() {
        let m = obj(json!({
            "@markdown": { "lines": [4], "raw": "./b.md" },
            "@frontmatter": { "lines": [2] },
            "_graphs": ["@frontmatter", "@markdown"]
        }));
        let out = filter_metadata(&m, &["@markdown".into()], &["lines".into()]);
        assert_eq!(out, obj(json!({ "@markdown": { "lines": [4] } })));
    }

    #[test]
    fn filtered_out_only_when_a_filter_is_set() {
        let empty = Map::new();
        assert!(!filtered_out(&empty, &[], &[]));
        assert!(filtered_out(&empty, &["@markdown".into()], &[]));
        assert!(filtered_out(&empty, &[], &["lines".into()]));
    }

    #[test]
    fn render_value_escapes_control_chars() {
        let mut lines = Vec::new();
        push_metadata_lines(
            &mut lines,
            &obj(json!({ "@frontmatter": { "note": "a\nb", "n": 2, "tags": ["x"] } })),
        );
        assert_eq!(
            lines,
            vec![
                "  @frontmatter".to_string(),
                "    n: 2".to_string(),
                "    note: \"a\\nb\"".to_string(),
                "    tags: [\"x\"]".to_string(),
            ]
        );
    }

    #[test]
    fn join_blocks_envelope() {
        assert_eq!(join_blocks(vec![]), "");
        assert_eq!(join_blocks(vec!["a".into(), "b".into()]), "a\n\nb\n");
    }
}
