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
        // Frontmatter is the frontmatter parser's to read. Masked rather than
        // skipped so byte offsets — and the link line numbers taken from them —
        // stay file-accurate.
        let masked = mask_frontmatter(content);
        let (links, anchors) = extract(masked.as_deref().unwrap_or(content));
        ParseResult {
            links,
            anchors,
            metadata: None,
        }
    }
}

/// Blank a leading YAML frontmatter block, preserving every newline so line
/// numbers are unchanged. `None` when the file opens with no well-formed block.
///
/// Without this, a single-key block reads as a **setext heading**: `purpose: x`
/// followed by the closing `---` is a paragraph underlined by dashes, so the
/// document publishes `#purpose-x` as an address it does not answer to — a
/// fabricated anchor that also silently accepts any link written to it.
fn mask_frontmatter(content: &str) -> Option<String> {
    let rest = content.strip_prefix("---")?;
    // The opening fence has to be a line of its own.
    if !rest.starts_with('\n') && !rest.starts_with("\r\n") {
        return None;
    }
    let close = 3 + rest.find("\n---")? + "\n---".len();
    let masked = content
        .char_indices()
        .map(|(i, ch)| {
            if i < close && ch != '\n' && ch != '\r' {
                ' '
            } else {
                ch
            }
        })
        .collect();
    Some(masked)
}

/// Walk the document once, collecting link targets and the anchors it defines.
///
/// Anchors come from two places, both of which a reader's platform resolves:
/// a heading, through the slug of its **rendered** text, and a raw
/// `<a id>`/`<a name>` element, verbatim. Heading text excludes image alt text,
/// because the alt attribute is not part of an element's text content and so is
/// not part of what gets slugged.
fn extract(content: &str) -> (Vec<Link>, Vec<String>) {
    let newlines = newline_offsets(content);
    let mut links = Vec::new();
    let mut anchors = Vec::new();
    let mut slugger = slug::Slugger::default();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    // The offset iterator yields each event's byte range so we can locate links.
    let parser = CmarkParser::new_ext(content, options).into_offset_iter();

    let mut in_code_block = false;
    // `Some` while inside a heading, accumulating its rendered text.
    let mut heading: Option<String> = None;
    // Nesting depth of image tags, whose alt text is not rendered text.
    let mut in_image = 0usize;

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
                    anchors.push(slugger.heading(&text));
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(buffer) = &mut heading
                    && in_image == 0
                {
                    buffer.push_str(&text);
                }
            }
            // A line break renders as markup, not as a space, so it contributes
            // no character to the text content. The slug drops it either way;
            // pushing a space would join the halves with a hyphen instead.
            Event::SoftBreak | Event::HardBreak => {
                if let Some(buffer) = &mut heading {
                    buffer.push('\n');
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if !in_code_block {
                    anchors.extend(html_anchor_ids(&html));
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                in_image += 1;
                if !in_code_block {
                    let link = dest_url.trim();
                    if !link.is_empty() {
                        links.push(Link {
                            target: link.to_string(),
                            line: Some(line_at(range.start, &newlines)),
                        });
                    }
                }
            }
            Event::End(TagEnd::Image) => in_image = in_image.saturating_sub(1),
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
            _ => {}
        }
    }

    // An address the document answers to more than once — an `<a id>` beside the
    // heading whose slug matches it — is still one address. The slugger already
    // keeps heading slugs distinct, so this only collapses a genuine repeat.
    let mut seen = std::collections::HashSet::new();
    anchors.retain(|anchor| seen.insert(anchor.clone()));

    (links, anchors)
}

/// The ids declared by `<a id="…">` / `<a name="…">` in a chunk of raw HTML.
///
/// GitHub resolves both, so a document using them really does answer to those
/// fragments and drft has to see them — a hand-rolled table of contents and a
/// back-compat anchor kept after a heading rename are the usual sources. Values
/// keep their case: GitHub prefixes the rendered id but matches the fragment as
/// written, so `#FAQ` and `#faq` are different addresses.
fn html_anchor_ids(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut ids = Vec::new();
    let mut search = 0;

    while let Some(found) = lower[search..].find("<a") {
        let after_name = search + found + "<a".len();
        search = after_name;
        // `<abbr>` is not `<a>`: the tag name has to end here.
        if !lower[after_name..].starts_with([' ', '\t', '\n', '\r', '/', '>']) {
            continue;
        }
        let Some(close) = lower[after_name..].find('>') else {
            break;
        };
        let end = after_name + close;
        for key in ["id", "name"] {
            if let Some(id) = attribute(&html[after_name..end], &lower[after_name..end], key) {
                ids.push(id);
            }
        }
        search = end;
    }

    ids
}

/// The value of attribute `key` in a tag's attribute text, or `None`.
///
/// `raw` and `lower` are the same span, cased and lowercased; the name is matched
/// against `lower` and the value read from `raw`. The name has to be whole —
/// preceded by whitespace or the span's start — so `data-id=` does not answer to
/// `id`.
fn attribute(raw: &str, lower: &str, key: &str) -> Option<String> {
    let mut search = 0;
    while let Some(found) = lower[search..].find(key) {
        let at = search + found;
        search = at + key.len();
        let whole = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let Some(rest) = lower[search..].trim_start().strip_prefix('=') else {
            continue;
        };
        if !whole {
            continue;
        }
        let value = &raw[raw.len() - rest.len()..].trim_start();
        let mut chars = value.chars();
        return match chars.next()? {
            quote @ ('"' | '\'') => {
                let end = value[1..].find(quote)?;
                Some(value[1..1 + end].to_string())
            }
            _ => {
                let end = value
                    .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .unwrap_or(value.len());
                (end > 0).then(|| value[..end].to_string())
            }
        };
    }
    None
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
        // The `<a id>` is not slugged as text — it declares an address of its own.
        // Here both routes name `obs-92`, which is one address, not two.
        assert_eq!(anchors("## <a id=\"obs-92\"></a>OBS-92\n"), vec!["obs-92"]);
        // When they differ, the file answers to both.
        assert_eq!(
            anchors("## <a id=\"legacy\"></a>OBS-92\n"),
            vec!["legacy", "obs-92"]
        );
    }

    #[test]
    fn html_anchors_outside_headings_are_addresses_too() {
        // GitHub resolves `<a id>` and `<a name>` wherever they sit, so a
        // hand-rolled table of contents target is a real address.
        assert_eq!(
            anchors("<a id=\"faq\"></a>\n\n## Questions\n\n<a name=\"legacy\"></a>\n"),
            vec!["faq", "questions", "legacy"]
        );
    }

    #[test]
    fn html_anchor_ids_keep_their_case_and_reject_lookalikes() {
        // GitHub matches the fragment as written, so `#FAQ` and `#faq` differ.
        assert_eq!(anchors("<a id=\"FAQ\"></a>\n"), vec!["FAQ"]);
        // `<abbr>` is not `<a>`, and `data-id` is not `id`.
        assert!(anchors("<abbr id=\"x\">a</abbr>\n").is_empty());
        assert!(anchors("<a data-id=\"x\"></a>\n").is_empty());
    }

    #[test]
    fn frontmatter_is_not_a_setext_heading() {
        // A single-key block's last line plus the closing `---` reads as a setext
        // heading to a bare parser, publishing an address the file does not have.
        let content = "---\npurpose: the design of the widget\n---\n\n# Real\n";
        assert_eq!(anchors(content), vec!["real"]);
    }

    #[test]
    fn masking_frontmatter_keeps_link_lines_accurate() {
        let parser = MarkdownParser { file_filter: None };
        let content = "---\ntitle: Doc\n---\n\n# T\n\nSee [setup](setup.md).\n";
        let links = parser.parse("t.md", content).links;
        assert_eq!(
            links[0].line,
            Some(7),
            "masking must not shift line numbers"
        );
    }

    #[test]
    fn image_alt_text_is_not_part_of_the_slug() {
        // Alt text is an attribute, not text content, so GitHub does not slug it.
        let parser = MarkdownParser { file_filter: None };
        let result = parser.parse("t.md", "# ![logo](logo.png) Project\n");
        assert_eq!(result.anchors, vec!["-project"]);
        assert_eq!(
            result.links[0].target, "logo.png",
            "the image is still an edge"
        );
    }

    #[test]
    fn a_line_break_in_a_setext_heading_contributes_no_character() {
        // The break renders as markup, so the halves join with nothing between.
        assert_eq!(
            anchors("Line one\nline two\n---\n"),
            vec!["line-oneline-two"]
        );
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
