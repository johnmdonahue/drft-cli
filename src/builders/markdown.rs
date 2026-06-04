//! The `markdown` builder: emits link edges from `[text](path)` body links. It
//! contributes no node metadata — cross-graph linkage to `fs` nodes happens at
//! compose by path coincidence.

use globset::GlobSet;

use crate::builders::link_edge;
use crate::model::Graph;
use crate::parsers::Parser;
use crate::parsers::markdown::MarkdownParser;

/// Build the `markdown` graph fragment from text files, labeled `label`.
/// `filter` scopes which paths the builder reads (`None` reads all). The
/// fragment carries only edges.
pub fn build(label: &str, texts: &[(String, String)], filter: Option<GlobSet>) -> Graph {
    let parser = MarkdownParser {
        file_filter: filter,
    };
    let mut graph = Graph::labeled(label);

    for (path, content) in texts {
        if !parser.matches(path) {
            continue;
        }
        for raw in parser.parse(path, content).links {
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
    fn emits_resolved_edges() {
        let t = texts(&[("docs/index.md", "[setup](setup.md) and [faq](../faq.md)")]);
        let graph = build("markdown", &t, None);
        assert_eq!(graph.label.as_deref(), Some("markdown"));
        assert!(graph.nodes.is_empty(), "markdown contributes no nodes");
        let targets: Vec<&str> = graph.edges.iter().map(|e| e.target.as_str()).collect();
        assert_eq!(targets, vec!["docs/setup.md", "faq.md"]);
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
