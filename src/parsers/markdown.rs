use super::{Link, ParseResult, Parser, slug};
use pulldown_cmark::{Event, LinkType, Options, Parser as CmarkParser, Tag, TagEnd};

/// Built-in markdown parser. Extracts inline/reference/autolinks and images.
pub struct MarkdownParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
}

impl Parser for MarkdownParser {
    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true, // No filter = receives all File nodes
        }
    }

    fn parse(&self, _path: &str, content: &str) -> ParseResult {
        let (links, headings) = extract(content);
        ParseResult {
            links,
            anchors: slug::anchors(headings.iter().map(String::as_str)),
            metadata: None,
        }
    }
}

/// Walk the document once, collecting link targets and heading text.
///
/// Heading text is the *rendered* text — the concatenated text and code-span
/// content between a heading's start and end — because that is what GitHub
/// slugs. Inline HTML is excluded for the same reason, so an `<a id>` written
/// beside a heading does not leak into the anchor it would override. A link
/// inside a heading contributes both an edge and its text.
fn extract(content: &str) -> (Vec<Link>, Vec<String>) {
    let newlines = newline_offsets(content);
    let mut links = Vec::new();
    let mut headings = Vec::new();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    // The offset iterator yields each event's byte range so we can locate links.
    let parser = CmarkParser::new_ext(content, options).into_offset_iter();

    let mut in_code_block = false;
    // `Some` while inside a heading, accumulating its rendered text.
    let mut heading: Option<String> = None;

    for (event, range) in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            // The `id` on `Tag::Heading` carries a `{#custom}` attribute, which
            // GitHub ignores in favor of the slug. Honoring it here would mint an
            // anchor that resolves in drft and 404s for a reader.
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(text) = heading.take() {
                    headings.push(text);
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(buffer) = &mut heading {
                    buffer.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(buffer) = &mut heading {
                    buffer.push(' ');
                }
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) if !in_code_block => {
                if link_type == LinkType::Email {
                    continue;
                }
                let link = dest_url.trim();
                if !link.is_empty() {
                    links.push(Link {
                        target: link.to_string(),
                        line: Some(line_at(range.start, &newlines)),
                    });
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) if !in_code_block => {
                let link = dest_url.trim();
                if !link.is_empty() {
                    links.push(Link {
                        target: link.to_string(),
                        line: Some(line_at(range.start, &newlines)),
                    });
                }
            }
            _ => {}
        }
    }

    (links, headings)
}

/// Byte offsets of every `\n` in `content`, ascending — a reusable index for
/// turning a byte offset into a line number.
fn newline_offsets(content: &str) -> Vec<usize> {
    content
        .bytes()
        .enumerate()
        .filter(|(_, b)| *b == b'\n')
        .map(|(i, _)| i)
        .collect()
}

/// 1-based line containing byte `offset`, via the precomputed newline index.
fn line_at(offset: usize, newlines: &[usize]) -> usize {
    newlines.partition_point(|&nl| nl < offset) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Vec<String> {
        let parser = MarkdownParser { file_filter: None };
        parser
            .parse("test.md", content)
            .links
            .into_iter()
            .map(|l| l.target)
            .collect()
    }

    fn anchors(content: &str) -> Vec<String> {
        let parser = MarkdownParser { file_filter: None };
        parser.parse("test.md", content).anchors
    }

    #[test]
    fn collects_anchors_from_atx_and_setext_headings() {
        assert_eq!(
            anchors("# Title\n\n## OBS-92\n\nSetext\n------\n"),
            vec!["title", "obs-92", "setext"]
        );
    }

    #[test]
    fn a_hash_inside_a_code_fence_is_not_a_heading() {
        let content = "# Real\n\n```sh\n# not a heading\n```\n";
        assert_eq!(anchors(content), vec!["real"]);
    }

    #[test]
    fn heading_text_is_the_rendered_text() {
        // Emphasis markers and code-span backticks are not part of what GitHub
        // slugs; the text inside them is.
        assert_eq!(anchors("## The **fs** graph\n"), vec!["the-fs-graph"]);
        assert_eq!(anchors("## The `fs` graph\n"), vec!["the-fs-graph"]);
    }

    #[test]
    fn a_link_in_a_heading_contributes_its_text_and_its_edge() {
        let parser = MarkdownParser { file_filter: None };
        let result = parser.parse("t.md", "## See [config](config.md)\n");
        assert_eq!(result.anchors, vec!["see-config"]);
        assert_eq!(result.links[0].target, "config.md");
    }

    #[test]
    fn inline_html_in_a_heading_is_not_part_of_the_slug() {
        // GitHub slugs the rendered text, so an `<a id>` beside a heading does not
        // leak into the anchor. (It would *override* it on GitHub — a separate
        // feature, deliberately not honored yet.)
        assert_eq!(anchors("## <a id=\"obs-92\"></a>OBS-92\n"), vec!["obs-92"]);
    }

    #[test]
    fn a_repeated_heading_takes_the_disambiguator() {
        assert_eq!(
            anchors("## Notes\n\ntext\n\n## Notes\n"),
            vec!["notes", "notes-1"]
        );
    }

    #[test]
    fn a_curly_id_attribute_is_ignored() {
        // GitHub ignores `{#custom}`, so honoring it would mint a tool-only anchor
        // that 404s for a reader.
        // The braces are literal text to a parser without heading attributes
        // enabled, so they slug like any other punctuation — which is what a
        // reader on GitHub gets too.
        assert_eq!(anchors("## OBS-92 {#custom}\n"), vec!["obs-92-custom"]);
    }

    #[test]
    fn records_link_line_numbers() {
        let parser = MarkdownParser { file_filter: None };
        let content = "# Title\n\nSee [setup](setup.md).\n\nThen [faq](faq.md).\n";
        let links = parser.parse("t.md", content).links;
        assert_eq!(links[0].target, "setup.md");
        assert_eq!(links[0].line, Some(3));
        assert_eq!(links[1].target, "faq.md");
        assert_eq!(links[1].line, Some(5));
    }

    #[test]
    fn extracts_inline_links() {
        let links = parse("[setup](setup.md) and [faq](faq.md)");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "setup.md");
        assert_eq!(links[1], "faq.md");
    }

    #[test]
    fn preserves_fragments() {
        // Parser emits raw targets; graph builder strips fragments
        let links = parse("[setup](setup.md#installation)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "setup.md#installation");
    }

    #[test]
    fn emits_external_urls() {
        let links = parse("[google](https://google.com) and [local](setup.md)");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "https://google.com");
        assert_eq!(links[1], "setup.md");
    }

    #[test]
    fn emits_anchor_only() {
        // Parser emits raw targets; graph builder filters anchor-only links
        let links = parse("[section](#heading) and [local](setup.md)");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "#heading");
        assert_eq!(links[1], "setup.md");
    }

    #[test]
    fn skips_email_links() {
        let links = parse("Contact (<user@example.com>)");
        assert!(links.is_empty());
    }

    #[test]
    fn emits_mailto_links() {
        // mailto: from inline syntax is emitted raw; graph builder filters
        let links = parse("[email](mailto:user@example.com) and [local](setup.md)");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "mailto:user@example.com");
        assert_eq!(links[1], "setup.md");
    }

    #[test]
    fn extracts_image_links() {
        let links = parse("![diagram](assets/arch.png)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "assets/arch.png");
    }

    #[test]
    fn extracts_reference_links() {
        let links = parse("[setup][ref]\n\n[ref]: setup.md\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "setup.md");
    }

    #[test]
    fn no_filter_matches_everything() {
        let parser = MarkdownParser { file_filter: None };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("main.rs"));
        assert!(parser.matches("docs/guide.md"));
    }

    #[test]
    fn file_filter_restricts_matching() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("*.md").unwrap());
        builder.add(globset::Glob::new("*.mdx").unwrap());
        let parser = MarkdownParser {
            file_filter: Some(builder.build().unwrap()),
        };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("page.mdx"));
        assert!(!parser.matches("main.rs"));
    }
}
