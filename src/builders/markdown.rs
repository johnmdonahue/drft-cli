//! The `markdown` builder: emits link edges from `[text](path)` body links, plus
//! the `#fragment` addresses each file answers to. It is colocated — the anchors
//! are about its own file — so its drift rides the file's `fs` hash; it
//! contributes no hash of its own. Cross-graph linkage to `fs` nodes happens at
//! compose by path coincidence.

use globset::GlobSet;
use serde_json::Value;

use crate::builders::{LinkPolicy, link_edges};
use crate::model::{Graph, Metadata, Node};
use crate::parsers::Parser;
use crate::parsers::markdown::MarkdownParser;

/// Build the `markdown` graph fragment from text files, labeled `label`.
/// `filter` scopes which paths the builder reads (`None` reads all). The
/// fragment carries link edges plus a node per read file listing its `anchors`.
///
/// A file the builder read gets a node even when it defines no anchors, so the
/// empty array is a fact rather than a silence. The distinction is load-bearing:
/// a link to `notes.md#missing` where `notes.md` was read and has no headings is
/// a broken reference, while the same link into a file the markdown graph never
/// read is simply unknown, and only a present-but-empty `anchors` tells them
/// apart.
pub fn build(label: &str, texts: &[(String, String)], filter: Option<GlobSet>) -> Graph {
    let parser = MarkdownParser {
        file_filter: filter,
    };
    let mut graph = Graph::labeled(label);

    for (path, content) in texts {
        if !parser.matches(path) {
            continue;
        }
        let result = parser.parse(path, content);

        let mut metadata = Metadata::new();
        metadata.insert(
            "anchors".into(),
            Value::Array(result.anchors.into_iter().map(Value::String).collect()),
        );
        graph.set_node(path.clone(), Node::new(metadata));

        for edge in link_edges(path, &result.links, LinkPolicy::Body) {
            graph.add_edge(edge);
        }
    }

    graph.sort_edges();
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect()
    }

    #[test]
    fn emits_resolved_edges() {
        let t = texts(&[("docs/index.md", "[setup](setup.md) and [faq](../faq.md)")]);
        let graph = build("markdown", &t, None);
        assert_eq!(graph.label.as_deref(), Some("markdown"));
        let targets: Vec<&str> = graph.edges.iter().map(|e| e.target.as_str()).collect();
        assert_eq!(targets, vec!["docs/setup.md", "faq.md"]);
    }

    #[test]
    fn publishes_the_anchors_a_file_answers_to() {
        let t = texts(&[("log.md", "# Log\n\n## OBS-92\n\ntext\n\n## OBS-40\n")]);
        let graph = build("markdown", &t, None);
        assert_eq!(
            graph.nodes["log.md"].metadata["anchors"],
            serde_json::json!(["log", "obs-92", "obs-40"]),
            "document order, so a repeated slug's disambiguator is legible"
        );
    }

    #[test]
    fn a_read_file_with_no_headings_still_gets_an_empty_anchor_list() {
        // Present-and-empty is what separates "read, defines nothing" from "never
        // read", which is the difference between a broken fragment and an unknown
        // one.
        let t = texts(&[("plain.md", "just prose\n")]);
        let graph = build("markdown", &t, None);
        assert_eq!(
            graph.nodes["plain.md"].metadata["anchors"],
            serde_json::json!([])
        );
    }

    #[test]
    fn an_unread_file_gets_no_node() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("**/*.md").unwrap());
        let t = texts(&[("a.md", "# A"), ("notes.txt", "# Not markdown here")]);
        let graph = build("markdown", &t, Some(builder.build().unwrap()));
        assert!(graph.nodes.contains_key("a.md"));
        assert!(!graph.nodes.contains_key("notes.txt"));
    }

    #[test]
    fn filter_scopes_files() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("**/*.md").unwrap());
        let filter = builder.build().unwrap();

        let t = texts(&[("a.md", "[x](x.md)"), ("notes.txt", "[y](y.md)")]);
        let graph = build("markdown", &t, Some(filter));
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].source, "a.md");
    }
}
