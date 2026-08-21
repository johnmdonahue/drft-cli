use super::{Link, ParseResult, Parser, frontmatter, slug};
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
/// numbers are unchanged. `None` when the file opens with no such block.
///
/// Without this, a single-key block reads as a **setext heading**: `purpose: x`
/// followed by the closing `---` is a paragraph underlined by dashes, so the
/// document publishes `#purpose-x` as an address it does not answer to — a
/// fabricated anchor that also silently accepts any link written to it.
///
/// The boundary comes from the frontmatter parser rather than being re-derived
/// here. Guessing it a second time is how the two disagree, and the failure is
/// not symmetric: masking a document that merely opens with a `---` thematic
/// break deletes its headings **and its links** from the graph, which reaches
/// staleness and `drft impact`, not just this parser's anchors.
fn mask_frontmatter(content: &str) -> Option<String> {
    let close = frontmatter::mapping_block_end(content)?;
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
///
/// Attributes are tokenized rather than searched for by name. Searching finds
/// `name=` inside a quoted value and mints it as an attribute, and takes the
/// first `>` as the tag end even when it sits inside a value — so
/// `<a title="a > b" id="x">` would lose a real address.
fn html_anchor_ids(html: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // `<a` has to be the whole tag name: `<abbr>` is a different element.
        if bytes[i] != b'<'
            || !html[i + 1..].starts_with(['a', 'A'])
            || !html[i + 2..].starts_with([' ', '\t', '\n', '\r', '/', '>'])
        {
            i += 1;
            continue;
        }
        i += 2;
        // HTML keeps the first of a repeated attribute, so a second `id` names an
        // address the page does not have.
        let (mut seen_id, mut seen_name) = (false, false);
        while let Some((name, value, next)) = attribute(html, i) {
            i = next;
            if value.is_empty() {
                continue;
            }
            let taken = match () {
                _ if name.eq_ignore_ascii_case("id") => &mut seen_id,
                _ if name.eq_ignore_ascii_case("name") => &mut seen_name,
                _ => continue,
            };
            if !std::mem::replace(taken, true) {
                ids.push(value);
            }
        }
    }

    ids
}

/// The next attribute in a tag body starting at `from`, as `(name, value, next)`.
/// `None` at the tag's `>` or at the end of the input, which is what ends the
/// caller's loop.
fn attribute(html: &str, from: usize) -> Option<(&str, String, usize)> {
    let bytes = html.as_bytes();
    let mut i = from;

    while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] == b'>' {
        return None;
    }

    let name_start = i;
    while i < bytes.len()
        && !bytes[i].is_ascii_whitespace()
        && !matches!(bytes[i], b'=' | b'>' | b'/')
    {
        i += 1;
    }
    let name = &html[name_start..i];

    // A bare attribute (`<a hidden>`) has no value; the caller skips it.
    let mut after_name = i;
    while after_name < bytes.len() && bytes[after_name].is_ascii_whitespace() {
        after_name += 1;
    }
    if after_name >= bytes.len() || bytes[after_name] != b'=' {
        return Some((name, String::new(), i));
    }
    i = after_name + 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() {
        return Some((name, String::new(), i));
    }

    let value_start;
    let value_end;
    match bytes[i] {
        quote @ (b'"' | b'\'') => {
            value_start = i + 1;
            match html[value_start..].find(quote as char) {
                Some(offset) => {
                    value_end = value_start + offset;
                    i = value_end + 1;
                }
                // An unterminated quote runs to the end of the chunk; taking the
                // rest as a value would mint an address out of prose.
                None => return None,
            }
        }
        _ => {
            value_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                i += 1;
            }
            value_end = i;
        }
    }

    Some((name, html[value_start..value_end].to_string(), i))
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
    fn a_quoted_angle_bracket_does_not_end_the_tag() {
        // Searching for the first `>` would truncate the tag and lose a real
        // address, turning every link to it into a false positive.
        assert_eq!(
            anchors("<a title=\"a > b\" id=\"kept\"></a>\n"),
            vec!["kept"]
        );
    }

    #[test]
    fn an_attribute_value_is_not_rescanned_for_attribute_names() {
        // `name=` inside a quoted value is text, not an attribute.
        assert_eq!(
            anchors("<a title=\"name=x\" id=\"only\"></a>\n"),
            vec!["only"]
        );
    }

    #[test]
    fn a_bare_attribute_and_an_empty_value_are_skipped() {
        assert_eq!(
            anchors("<a hidden id=\"after-bare\"></a>\n"),
            vec!["after-bare"]
        );
        assert!(
            anchors("<a id=\"\"></a>\n").is_empty(),
            "empty is not an address"
        );
    }

    #[test]
    fn a_malformed_tag_mints_nothing() {
        // The parser's own tag grammar rejects it before the scan, and the scan
        // bails on an unterminated quote rather than taking prose as a value.
        assert!(anchors("<a id=\"unclosed name=\"real\"></a>\n").is_empty());
        assert!(anchors("<a id=\"never closed\n").is_empty());
    }

    #[test]
    fn a_repeated_attribute_names_one_address() {
        // HTML keeps the first, so a second `id` is an address the page lacks.
        assert_eq!(
            anchors("<a id=\"first\" id=\"second\" name=\"nm\" name=\"nm2\"></a>\n"),
            vec!["first", "nm"]
        );
    }

    #[test]
    fn a_code_span_in_frontmatter_does_not_leak_an_anchor() {
        // A backtick span can hide a `:` that breaks the mapping. The frontmatter
        // parser falls back to a code-masked parse, and the mask has to agree or
        // the fabricated-anchor bug comes back for exactly this file.
        let content = "---\npurpose: use `a: b` here\n---\n\n# Real\n";
        assert_eq!(anchors(content), vec!["real"]);
    }

    #[test]
    fn a_comment_only_block_is_read_as_content() {
        // A YAML comment and a markdown ATX heading are the same syntax, so
        // accepting a comment-only block would delete `# First` from any document
        // that opens with a thematic break above a heading.
        assert_eq!(
            anchors("---\n# just a comment\n---\n\n# Real\n"),
            vec!["just-a-comment", "real"]
        );
    }

    #[test]
    fn a_leading_thematic_break_is_not_frontmatter() {
        // Masking a document that merely opens with `---` deletes its headings
        // *and* its links from the graph, which reaches far past this parser.
        let parser = MarkdownParser { file_filter: None };
        let content = "---\n\n# First\n\nSee [a](t.md).\n\n---\n\n# Second\n";
        let result = parser.parse("hr.md", content);
        assert_eq!(result.anchors, vec!["first", "second"]);
        assert_eq!(result.links[0].target, "t.md");
        assert_eq!(result.links[0].line, Some(5));
    }

    #[test]
    fn a_rule_above_a_setext_title_is_not_frontmatter() {
        assert_eq!(anchors("---\nMy Title\n---\n\nBody\n"), vec!["my-title"]);
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
