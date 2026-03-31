use super::{ParseResult, Parser};
use pulldown_cmark::{Event, LinkType, Options, Parser as CmarkParser, Tag, TagEnd};

/// Built-in markdown parser. Extracts inline/reference/autolinks and images.
pub struct MarkdownParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
}

impl Parser for MarkdownParser {
    fn name(&self) -> &str {
        "markdown"
    }

    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true, // No filter = receives all File nodes
        }
    }

    fn parse(&self, _path: &str, content: &str) -> ParseResult {
        ParseResult {
            links: extract_markdown_links(content),
            metadata: None,
        }
    }
}

fn extract_markdown_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = CmarkParser::new_ext(content, options);

    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
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
                    links.push(link.to_string());
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) if !in_code_block => {
                let link = dest_url.trim();
                if !link.is_empty() {
                    links.push(link.to_string());
                }
            }
            _ => {}
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Vec<String> {
        let parser = MarkdownParser { file_filter: None };
        parser.parse("test.md", content).links
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
