//! `drft edges`: project the graph's edges — the read verb for "what does this
//! file link to". Matched on `edge.source`, so the selector picks source nodes and
//! the projection is every edge leaving them: the outbound one-hop view, distinct
//! from `impact` (a transitive traversal from a seed set) and `check` (a
//! whole-graph gate).
//!
//! Like `nodes`, this is a *reader*: selectors expand safely, and each edge's
//! metadata is narrowed by `--namespace`/`--field` through the shared
//! [`crate::projection`] helpers, so the two verbs stay consistent. Selector-to-key
//! resolution and namespace validation live in `main.rs`.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::model::Graph;
use crate::projection;

/// A projected edge: its endpoints and the namespaced metadata that survived the
/// namespace/field filters. `metadata` holds only `@<graph>` blocks (`lines`,
/// `raw`, …); the `_graphs` provenance is dropped. An edge with no per-graph
/// metadata projects with an empty object — it is still an edge.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EdgeProjection {
    pub source: String,
    pub target: String,
    pub metadata: Map<String, Value>,
}

/// Project the edges whose `source` is in `sources`, narrowing each edge's metadata
/// to `namespaces`/`fields`. Iterates `graph.edges`, which compose sorts by
/// `(source, target)`, so the result is deterministic.
///
/// An edge drops out only when a namespace or field filter is set and the edge
/// carries none of it; with no filter, an edge with no per-graph metadata is kept.
pub fn project(
    graph: &Graph,
    sources: &[String],
    namespaces: &[String],
    fields: &[String],
) -> Vec<EdgeProjection> {
    let sources: HashSet<&str> = sources.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for edge in &graph.edges {
        if !sources.contains(edge.source.as_str()) {
            continue;
        }
        let metadata = projection::filter_metadata(&edge.metadata, namespaces, fields);
        if projection::filtered_out(&metadata, namespaces, fields) {
            continue;
        }
        out.push(EdgeProjection {
            source: edge.source.clone(),
            target: edge.target.clone(),
            metadata,
        });
    }
    out
}

/// Render projected edges as a compact, LLM-legible block per edge: `source →
/// target` on the header line, each namespace indented under it, each field under
/// that. Blocks are separated by a blank line and the output ends in a newline; an
/// empty projection renders to the empty string.
pub fn format_text(edges: &[EdgeProjection]) -> String {
    let mut blocks = Vec::new();
    for edge in edges {
        let mut lines = vec![format!("{} → {}", edge.source, edge.target)];
        projection::push_metadata_lines(&mut lines, &edge.metadata);
        blocks.push(lines.join("\n"));
    }
    projection::join_blocks(blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Edge;
    use serde_json::json;

    fn obj(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn sample_graph() -> Graph {
        let mut g = Graph::composed();
        // a.md links b.md (markdown, with a line) and c.md (a bare edge, no metadata).
        g.add_edge(Edge::with_metadata(
            "a.md",
            "b.md",
            obj(
                json!({ "@markdown": { "lines": [4], "raw": "./b.md" }, "_graphs": ["@markdown"] }),
            ),
        ));
        g.add_edge(Edge::with_metadata(
            "a.md",
            "c.md",
            obj(json!({ "_graphs": ["@markdown"] })),
        ));
        // b.md links c.md (frontmatter).
        g.add_edge(Edge::with_metadata(
            "b.md",
            "c.md",
            obj(json!({ "@frontmatter": { "lines": [2] }, "_graphs": ["@frontmatter"] })),
        ));
        g.sort_edges();
        g
    }

    #[test]
    fn matches_on_source() {
        let g = sample_graph();
        let out = project(&g, &["a.md".into()], &[], &[]);
        let pairs: Vec<_> = out.iter().map(|e| (&e.source, &e.target)).collect();
        assert_eq!(
            pairs,
            vec![
                (&"a.md".into(), &"b.md".into()),
                (&"a.md".into(), &"c.md".into())
            ]
        );
    }

    #[test]
    fn bare_edge_keeps_empty_metadata_without_filter() {
        let g = sample_graph();
        let out = project(&g, &["a.md".into()], &[], &[]);
        let bare = out.iter().find(|e| e.target == "c.md").unwrap();
        assert!(
            bare.metadata.is_empty(),
            "no per-graph metadata, but still an edge"
        );
        assert!(!out.iter().any(|e| e.metadata.contains_key("_graphs")));
    }

    #[test]
    fn namespace_filter_drops_edges_without_the_lens() {
        let g = sample_graph();
        // Every edge leaving a.md, restricted to @frontmatter — none carry it.
        let out = project(&g, &["a.md".into()], &["@frontmatter".into()], &[]);
        assert!(out.is_empty());
        // b.md's edge does carry @frontmatter.
        let out = project(&g, &["b.md".into()], &["@frontmatter".into()], &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].metadata,
            obj(json!({ "@frontmatter": { "lines": [2] } }))
        );
    }

    #[test]
    fn field_filter_narrows_within_the_namespace() {
        let g = sample_graph();
        let out = project(
            &g,
            &["a.md".into()],
            &["@markdown".into()],
            &["lines".into()],
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target, "b.md");
        assert_eq!(
            out[0].metadata,
            obj(json!({ "@markdown": { "lines": [4] } }))
        );
    }

    #[test]
    fn format_text_is_source_arrow_target_then_metadata() {
        let g = sample_graph();
        let out = project(
            &g,
            &["a.md".into()],
            &["@markdown".into()],
            &["lines".into()],
        );
        assert_eq!(
            format_text(&out),
            "a.md → b.md\n  @markdown\n    lines: [4]\n"
        );
    }

    #[test]
    fn format_text_bare_edge_is_header_only() {
        let g = sample_graph();
        let out = project(&g, &["a.md".into()], &[], &[]);
        let text = format_text(&out);
        // The bare a.md → c.md edge renders as just its header, no indented lines.
        assert!(text.contains("a.md → c.md\n"), "got: {text}");
    }

    #[test]
    fn format_text_empty_is_blank() {
        assert_eq!(format_text(&[]), "");
    }
}
