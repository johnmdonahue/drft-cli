//! The `frontmatter` builder: emits link edges from frontmatter link-target
//! values, plus the parsed frontmatter block as metadata on the file it read.
//! It is colocated — its metadata is about its own file — so its drift rides the
//! file's `fs` hash; it contributes no hash of its own.

use globset::GlobSet;

use crate::builders::link_edge;
use crate::model::{Graph, Node};
use crate::parsers::Parser;
use crate::parsers::frontmatter::FrontmatterParser;

/// Build the `frontmatter` graph fragment from text files. `filter` scopes which
/// paths the builder reads (`None` reads all). The fragment carries edges plus a
/// node per file whose frontmatter parses to an object.
pub fn build(texts: &[(String, String)], filter: Option<GlobSet>) -> Graph {
    let parser = FrontmatterParser {
        file_filter: filter,
    };
    let mut graph = Graph::labeled("frontmatter");

    for (path, content) in texts {
        if !parser.matches(path) {
            continue;
        }
        let result = parser.parse(path, content);

        if let Some(serde_json::Value::Object(block)) = result.metadata {
            graph.set_node(path.clone(), Node::new(block));
        }

        for raw in result.links {
            if let Some(edge) = link_edge(path, &raw) {
                graph.add_edge(edge);
            }
        }
    }

    graph.edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.target.cmp(&b.target))
    });
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
    fn emits_metadata_and_edges() {
        let t = texts(&[(
            "analysis.md",
            "---\ntitle: Analysis\nstatus: draft\nsources:\n  - ./data/notes.md\n---\n\n# Body\n",
        )]);
        let graph = build(&t, None);
        assert_eq!(graph.label.as_deref(), Some("frontmatter"));

        let meta = &graph.nodes["analysis.md"].metadata;
        assert_eq!(meta["title"], serde_json::json!("Analysis"));
        assert_eq!(meta["status"], serde_json::json!("draft"));

        let targets: Vec<&str> = graph.edges.iter().map(|e| e.target.as_str()).collect();
        assert_eq!(targets, vec!["data/notes.md"]);
    }

    #[test]
    fn no_node_without_frontmatter() {
        let t = texts(&[("plain.md", "# Just a heading\n")]);
        let graph = build(&t, None);
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
    }
}
