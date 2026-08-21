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
            let filtered = filter_fields(obj, fields);
            if filtered.is_empty() {
                continue;
            }
            Value::Object(filtered)
        };
        out.insert(ns.clone(), block);
    }
    out
}

/// Narrow one namespace block to `fields`. A key whose own name matches is kept
/// whole. A key holding an array of objects is kept with each entry narrowed the
/// same way, so a field names what it is wherever it is declared: `--field line`
/// reaches an edge's per-occurrence facts, and `--field role` reaches an authored
/// frontmatter list, without the caller naming the array it lives in. An entry
/// that keeps nothing drops out, and a key whose entries all drop out goes with
/// them — so the descent also decides whether a node survives `filtered_out`.
fn filter_fields(block: &Map<String, Value>, fields: &[String]) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in block {
        if fields.iter().any(|f| f == key) {
            out.insert(key.clone(), value.clone());
            continue;
        }
        let Some(entries) = object_entries(value) else {
            continue;
        };
        let narrowed: Vec<Value> = entries
            .into_iter()
            .map(|entry| filter_fields(entry, fields))
            .filter(|entry| !entry.is_empty())
            .map(Value::Object)
            .collect();
        if !narrowed.is_empty() {
            out.insert(key.clone(), Value::Array(narrowed));
        }
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
            Some(obj) => push_fields(lines, obj, 4),
            None => lines.push(format!("    {}", render_value(value))),
        }
    }
}

/// Render one object's fields at `indent` spaces, one `key: value` per line.
///
/// A field holding an array of objects — an edge's `occurrences`, or an authored
/// frontmatter list — renders as a bare header line followed by a `- `-marked
/// sub-block per entry, so each entry's fields keep their own lines. Rendering it
/// through [`render_value`] instead would collapse the whole array onto one long
/// JSON line, which is the opposite of what the text shape is for.
///
/// An entry with no fields renders as `- {}`. It has no key to carry the marker,
/// so emitting nothing would drop it from the rendering and silently misreport
/// how many entries the array holds.
fn push_fields(lines: &mut Vec<String>, obj: &Map<String, Value>, indent: usize) {
    let pad = " ".repeat(indent);
    for (key, value) in obj {
        let Some(entries) = object_entries(value) else {
            lines.push(format!("{pad}{key}: {}", render_value(value)));
            continue;
        };
        lines.push(format!("{pad}{key}"));
        for entry in entries {
            if entry.is_empty() {
                lines.push(format!("{pad}  - {{}}"));
                continue;
            }
            for (i, (k, v)) in entry.iter().enumerate() {
                let marker = if i == 0 { "- " } else { "  " };
                lines.push(format!("{pad}  {marker}{k}: {}", render_value(v)));
            }
        }
    }
}

/// The entries of an array whose every element is an object, with at least one
/// carrying a field. Anything else — a scalar, a mixed array, an array of empty
/// objects — returns `None` so the caller falls back to rendering the value whole.
fn object_entries(value: &Value) -> Option<Vec<&Map<String, Value>>> {
    let entries: Vec<&Map<String, Value>> = value
        .as_array()?
        .iter()
        .map(Value::as_object)
        .collect::<Option<Vec<_>>>()?;
    entries.iter().any(|e| !e.is_empty()).then_some(entries)
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

/// Join labeled sections for the composed `graph` text view. Each section is a
/// `# <label>` header followed by its already-rendered body (from `join_blocks`),
/// with one blank line between the header and a non-empty body and one blank line
/// between sections. An empty body renders as its header alone — the section is
/// still named, so a graph with no edges reads as `# edges` with nothing under it
/// rather than a missing or dangling section. Metadata lines are always indented,
/// so they never look like a marker; a flush-left content line could in principle
/// collide (a file literally named `# edges` would render one), but no ordinary
/// path or `source → target` header begins with `# `, so in practice the markers
/// stand out.
pub fn join_sections(sections: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (i, (label, body)) in sections.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("# {label}\n"));
        if !body.is_empty() {
            out.push('\n');
            out.push_str(body);
        }
    }
    out
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
        let m = obj(json!({
            "@markdown": { "occurrences": [{ "line": 4 }] },
            "_graphs": ["@markdown"]
        }));
        let out = filter_metadata(&m, &[], &[]);
        assert_eq!(out.keys().collect::<Vec<_>>(), vec!["@markdown"]);
    }

    #[test]
    fn filter_by_namespace_and_field() {
        let m = obj(json!({
            "@frontmatter": { "purpose": "how the walk is scoped", "status": "draft" },
            "@fs": { "type": "file", "hash": "b3:1" },
            "_graphs": ["@frontmatter", "@fs"]
        }));
        let out = filter_metadata(&m, &["@frontmatter".into()], &["purpose".into()]);
        assert_eq!(
            out,
            obj(json!({ "@frontmatter": { "purpose": "how the walk is scoped" } }))
        );
    }

    #[test]
    fn filter_reaches_into_occurrence_entries() {
        // `--field line` narrows each occurrence without the caller naming the
        // array it lives in.
        let m = obj(json!({
            "@markdown": { "occurrences": [
                { "line": 53, "link": "b.md#x", "raw": "./b.md" },
                { "line": 77, "link": "b.md#y", "raw": "./b.md" },
            ] },
            "_graphs": ["@markdown"],
        }));
        let out = filter_metadata(&m, &[], &["line".into()]);
        assert_eq!(
            out["@markdown"],
            json!({ "occurrences": [{ "line": 53 }, { "line": 77 }] })
        );
    }

    #[test]
    fn filter_keeps_a_named_array_whole() {
        let m = obj(json!({ "@markdown": { "occurrences": [{ "line": 4 }] } }));
        let out = filter_metadata(&m, &[], &["occurrences".into()]);
        assert_eq!(out["@markdown"], json!({ "occurrences": [{ "line": 4 }] }));
    }

    #[test]
    fn filter_drops_a_block_matching_at_neither_level() {
        let m = obj(json!({ "@markdown": { "occurrences": [{ "line": 4 }] } }));
        assert!(filter_metadata(&m, &[], &["purpose".into()]).is_empty());
    }

    #[test]
    fn occurrences_render_one_entry_per_sub_block() {
        // The array must not collapse onto one JSON line — the text shape exists
        // so a reader can see which line cites which anchor.
        let mut lines = Vec::new();
        push_metadata_lines(
            &mut lines,
            &obj(json!({ "@markdown": { "occurrences": [
                { "line": 53, "link": "owners.md#security-console" },
                { "line": 77, "link": "owners.md#ngwaf-edge" },
            ] } })),
        );
        assert_eq!(
            lines,
            vec![
                "  @markdown",
                "    occurrences",
                "      - line: 53",
                "        link: owners.md#security-console",
                "      - line: 77",
                "        link: owners.md#ngwaf-edge",
            ]
        );
    }

    #[test]
    fn a_plain_array_still_renders_inline() {
        // Only an array of objects gets the sub-block treatment; a scalar array
        // keeps the one-line-per-field shape.
        let mut lines = Vec::new();
        push_metadata_lines(
            &mut lines,
            &obj(json!({ "@frontmatter": { "tags": ["graph", "core"] } })),
        );
        assert_eq!(
            lines,
            vec!["  @frontmatter", "    tags: [\"graph\",\"core\"]"]
        );
    }

    #[test]
    fn filter_reaches_into_an_authored_frontmatter_list() {
        // `filter_fields` is shared by `nodes` and `edges`, so the descent applies
        // to authored frontmatter too: a field names what it is wherever it is
        // declared, and a node declaring it only inside a list survives the filter.
        let m = obj(json!({ "@frontmatter": { "authors": [
            { "name": "Ana", "role": "lead" },
            { "role": "reviewer" },
        ] } }));
        let out = filter_metadata(&m, &[], &["role".into()]);
        assert_eq!(
            out["@frontmatter"],
            json!({ "authors": [{ "role": "lead" }, { "role": "reviewer" }] })
        );
        assert!(!filtered_out(&out, &[], &["role".into()]));
    }

    #[test]
    fn an_empty_entry_still_renders() {
        // An empty entry has no key to carry the `- ` marker. Emitting nothing
        // would drop it and misreport how many entries the array holds.
        let mut lines = Vec::new();
        push_metadata_lines(
            &mut lines,
            &obj(json!({ "@frontmatter": { "authors": [
                { "name": "Ana" },
                {},
                { "name": "Bo" },
            ] } })),
        );
        assert_eq!(
            lines,
            vec![
                "  @frontmatter",
                "    authors",
                "      - name: Ana",
                "      - {}",
                "      - name: Bo",
            ],
            "three entries in, three entries out"
        );
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

    #[test]
    fn join_sections_labels_both_and_separates_with_a_blank_line() {
        // Each body is what `join_blocks` yields — a trailing newline included.
        let nodes = join_blocks(vec!["docs/a.md".into()]);
        let edges = join_blocks(vec!["a.md → b.md".into()]);
        assert_eq!(
            join_sections(&[("nodes", &nodes), ("edges", &edges)]),
            "# nodes\n\ndocs/a.md\n\n# edges\n\na.md → b.md\n"
        );
    }

    #[test]
    fn join_sections_keeps_an_empty_section_as_its_header() {
        // A graph with no edges: the `# edges` header still names the section, with
        // nothing under it and no dangling blank line.
        let nodes = join_blocks(vec!["docs/a.md".into()]);
        assert_eq!(
            join_sections(&[("nodes", &nodes), ("edges", "")]),
            "# nodes\n\ndocs/a.md\n\n# edges\n"
        );
    }

    #[test]
    fn join_sections_empty_graph_is_both_headers() {
        // A graph with no nodes and no edges: both headers, nothing under either.
        assert_eq!(
            join_sections(&[("nodes", ""), ("edges", "")]),
            "# nodes\n\n# edges\n"
        );
    }

    #[test]
    fn join_sections_empty_nodes_with_edges() {
        // An empty leading section still gets its header, then the populated one.
        let edges = join_blocks(vec!["a.md → b.md".into()]);
        assert_eq!(
            join_sections(&[("nodes", ""), ("edges", &edges)]),
            "# nodes\n\n# edges\n\na.md → b.md\n"
        );
    }
}
