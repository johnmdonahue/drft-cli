use super::{ParseResult, Parser, RawLink};
use pulldown_cmark::{Event, LinkType, Options, Parser as CmarkParser, Tag, TagEnd};

/// Built-in markdown parser. Extracts inline/reference/autolinks, images,
/// frontmatter file references, and wikilinks.
pub struct MarkdownParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
    pub type_filter: Option<Vec<String>>,
    /// When true, parse YAML frontmatter and return as node metadata.
    pub extract_metadata: bool,
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
        let mut links = Vec::new();

        links.extend(extract_frontmatter(content));
        links.extend(extract_wikilinks(content));
        links.extend(extract_markdown_links(content));

        // Apply type filter if configured
        if let Some(ref types) = self.type_filter {
            links.retain(|l| types.iter().any(|t| t == &l.link_type));
        }

        let metadata = if self.extract_metadata {
            extract_frontmatter_metadata(content)
        } else {
            None
        };

        ParseResult { links, metadata }
    }
}

fn extract_markdown_links(content: &str) -> Vec<RawLink> {
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
                if let Some(link) = process_link(&dest_url, map_link_type(link_type)) {
                    links.push(link);
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) if !in_code_block => {
                if let Some(link) = process_link(&dest_url, "image") {
                    links.push(link);
                }
            }
            _ => {}
        }
    }

    links
}

fn map_link_type(lt: LinkType) -> &'static str {
    match lt {
        LinkType::Inline => "inline",
        LinkType::Reference | LinkType::ReferenceUnknown => "reference",
        LinkType::Collapsed | LinkType::CollapsedUnknown => "reference",
        LinkType::Shortcut | LinkType::ShortcutUnknown => "reference",
        LinkType::Autolink | LinkType::Email => "autolink",
    }
}

fn process_link(url: &str, link_type: &str) -> Option<RawLink> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // External URLs — record as external, don't skip
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(RawLink {
            target: url.to_string(),
            link_type: link_type.to_string(),
            is_external: true,
        });
    }

    // Skip mailto links
    if url.starts_with("mailto:") {
        return None;
    }

    // Skip anchor-only links
    if url.starts_with('#') {
        return None;
    }

    // Strip fragment
    let target = match url.find('#') {
        Some(idx) => &url[..idx],
        None => url,
    };

    if target.is_empty() {
        return None;
    }

    Some(RawLink {
        target: target.to_string(),
        link_type: link_type.to_string(),
        is_external: false,
    })
}

/// Extract file path references from YAML frontmatter.
/// Operates on code-block-stripped content to avoid parsing frontmatter
/// inside fenced code block examples.
fn extract_frontmatter(content: &str) -> Vec<RawLink> {
    let content = &strip_code(content);
    let mut links = Vec::new();

    if !content.starts_with("---") {
        return links;
    }

    let rest = &content[3..];
    let end = match rest.find("\n---") {
        Some(idx) => idx,
        None => return links,
    };

    let frontmatter = &rest[..end];

    for line in frontmatter.lines() {
        let line = line.trim();

        let value = if let Some(stripped) = line.strip_prefix("- ") {
            stripped.trim()
        } else if let Some((_key, val)) = line.split_once(':') {
            val.trim()
        } else {
            continue;
        };

        if value.is_empty() {
            continue;
        }

        if value.starts_with('{')
            || value.starts_with('[')
            || value.starts_with('"')
            || value.starts_with('\'')
        {
            continue;
        }

        let looks_like_path =
            (value.contains('/') || value.starts_with("./")) && has_file_extension(value);

        if !looks_like_path {
            continue;
        }

        if value.starts_with("http://") || value.starts_with("https://") {
            continue;
        }

        links.push(RawLink {
            target: value.to_string(),
            link_type: "frontmatter".to_string(),
            is_external: false,
        });
    }

    links
}

/// Parse YAML frontmatter into a JSON value for node metadata.
/// Returns None if no valid frontmatter is found.
fn extract_frontmatter_metadata(content: &str) -> Option<serde_json::Value> {
    let content = &strip_code(content);

    if !content.starts_with("---") {
        return None;
    }

    let rest = &content[3..];
    let end = rest.find("\n---")?;
    let yaml_str = &rest[..end];

    if yaml_str.trim().is_empty() {
        return None;
    }

    match serde_yaml::from_str::<serde_yaml::Value>(yaml_str) {
        Ok(yaml_val) => Some(yaml_to_json(yaml_val)),
        Err(e) => {
            eprintln!("warn: markdown parser: invalid frontmatter YAML: {e}");
            None
        }
    }
}

/// Convert serde_yaml::Value to serde_json::Value.
fn yaml_to_json(yaml: serde_yaml::Value) -> serde_json::Value {
    match yaml {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.into_iter().map(yaml_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        other => serde_json::to_string(&yaml_to_json(other)).ok()?,
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

fn has_file_extension(path: &str) -> bool {
    if let Some(basename) = path.rsplit('/').next() {
        basename.contains('.')
    } else {
        path.contains('.')
    }
}

/// Strip all code content (fenced blocks and inline backtick spans),
/// replacing with spaces to preserve offsets.
fn strip_code(content: &str) -> String {
    // First strip fenced code blocks (``` and ~~~)
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;
    let mut fence_marker = "";

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !in_code_block {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = true;
                fence_marker = if trimmed.starts_with("```") {
                    "```"
                } else {
                    "~~~"
                };
                result.push_str(&" ".repeat(line.len()));
            } else {
                result.push_str(line);
            }
        } else if trimmed.starts_with(fence_marker) && trimmed.trim() == fence_marker {
            in_code_block = false;
            result.push_str(&" ".repeat(line.len()));
        } else {
            result.push_str(&" ".repeat(line.len()));
        }
        result.push('\n');
    }

    // Then strip inline code spans (single and double backticks)
    let mut cleaned = String::with_capacity(result.len());
    let chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            // Count opening backticks
            let mut ticks = 0;
            while i + ticks < chars.len() && chars[i + ticks] == '`' {
                ticks += 1;
            }
            // Find matching closing backticks in the char array
            let after = i + ticks;
            let mut found = None;
            let mut j = after;
            while j + ticks <= chars.len() {
                if chars[j..j + ticks].iter().all(|c| *c == '`') {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(close_start) = found {
                // Replace entire span (backticks + content + backticks) with spaces
                let total = close_start + ticks - i;
                for _ in 0..total {
                    cleaned.push(' ');
                }
                i += total;
            } else {
                // No closing — keep the backtick as-is
                cleaned.push(chars[i]);
                i += 1;
            }
        } else {
            cleaned.push(chars[i]);
            i += 1;
        }
    }

    cleaned
}

/// Extract wikilinks: [[page]] or [[page|display text]].
/// Skips content inside fenced code blocks.
fn extract_wikilinks(content: &str) -> Vec<RawLink> {
    let clean = strip_code(content);
    let mut links = Vec::new();
    let mut rest = clean.as_str();

    while let Some(start) = rest.find("[[") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("]]") {
            let inner = &after_open[..end];

            if !inner.is_empty() && !inner.contains('\n') {
                let page = match inner.find('|') {
                    Some(idx) => &inner[..idx],
                    None => inner,
                };
                let page = page.trim();

                if !page.is_empty() {
                    let target = if page.ends_with(".md") {
                        page.to_string()
                    } else {
                        format!("{page}.md")
                    };

                    links.push(RawLink {
                        target,
                        link_type: "wikilink".to_string(),
                        is_external: false,
                    });
                }
            }

            rest = &after_open[end + 2..];
        } else {
            break;
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> Vec<RawLink> {
        let parser = MarkdownParser {
            file_filter: None,
            type_filter: None,
            extract_metadata: false,
        };
        parser.parse("test.md", content).links
    }

    #[test]
    fn extracts_inline_links() {
        let links = parse("[setup](setup.md) and [faq](faq.md)");
        let local: Vec<_> = links.iter().filter(|l| !l.is_external).collect();
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].target, "setup.md");
        assert_eq!(local[0].link_type, "inline");
        assert_eq!(local[1].target, "faq.md");
    }

    #[test]
    fn strips_fragment() {
        let links = parse("[setup](setup.md#installation)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "setup.md");
    }

    #[test]
    fn captures_external_urls() {
        let links = parse("[google](https://google.com) and [local](setup.md)");
        assert_eq!(links.len(), 2);
        let external: Vec<_> = links.iter().filter(|l| l.is_external).collect();
        assert_eq!(external.len(), 1);
        assert_eq!(external[0].target, "https://google.com");
    }

    #[test]
    fn skips_anchor_only() {
        let links = parse("[section](#heading) and [local](setup.md)");
        let local: Vec<_> = links.iter().filter(|l| !l.is_external).collect();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].target, "setup.md");
    }

    #[test]
    fn skips_email_links() {
        let links = parse("Contact (<user@example.com>)");
        assert!(links.is_empty());
    }

    #[test]
    fn extracts_image_links() {
        let links = parse("![diagram](assets/arch.png)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "assets/arch.png");
        assert_eq!(links[0].link_type, "image");
    }

    #[test]
    fn extracts_reference_links() {
        let links = parse("[setup][ref]\n\n[ref]: setup.md\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "setup.md");
        assert_eq!(links[0].link_type, "reference");
    }

    #[test]
    fn extracts_frontmatter_sources() {
        let content =
            "---\nsources:\n  - ../shared/glossary.md\n  - ./prior-art.md\n---\n\n# Hello\n";
        let links = parse(content);
        let fm: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == "frontmatter")
            .collect();
        assert_eq!(fm.len(), 2);
        assert_eq!(fm[0].target, "../shared/glossary.md");
        assert_eq!(fm[1].target, "./prior-art.md");
    }

    #[test]
    fn frontmatter_skips_non_paths() {
        let content = "---\ntitle: My Document\nversion: 1.0\ntags:\n  - rust\n  - cli\n---\n";
        let links = parse(content);
        let fm: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == "frontmatter")
            .collect();
        assert!(fm.is_empty());
    }

    #[test]
    fn extracts_wikilinks() {
        let links = parse("See [[setup]] for details and [[guides/intro]].");
        let wl: Vec<_> = links.iter().filter(|l| l.link_type == "wikilink").collect();
        assert_eq!(wl.len(), 2);
        assert_eq!(wl[0].target, "setup.md");
        assert_eq!(wl[1].target, "guides/intro.md");
    }

    #[test]
    fn wikilink_with_display_text() {
        let links = parse("See [[setup|Setup Guide]] for details.");
        let wl: Vec<_> = links.iter().filter(|l| l.link_type == "wikilink").collect();
        assert_eq!(wl.len(), 1);
        assert_eq!(wl[0].target, "setup.md");
    }

    #[test]
    fn wikilink_skips_code_blocks() {
        let content = "See [[real]].\n\n```json\n{\"cmd\": \"[[ $FOO == *.md ]]\"}\n```\n\nAnd [[also-real]].\n";
        let links = parse(content);
        let wl: Vec<_> = links.iter().filter(|l| l.link_type == "wikilink").collect();
        assert_eq!(wl.len(), 2);
        assert_eq!(wl[0].target, "real.md");
        assert_eq!(wl[1].target, "also-real.md");
    }

    #[test]
    fn wikilink_skips_inline_code() {
        let content = "See [[real]] and `[[not-a-link]]` here. Also ``[[also-not]]`` end.\n";
        let links = parse(content);
        let wl: Vec<_> = links.iter().filter(|l| l.link_type == "wikilink").collect();
        assert_eq!(wl.len(), 1);
        assert_eq!(wl[0].target, "real.md");
    }

    #[test]
    fn frontmatter_skips_code_block_examples() {
        let content = "# Doc\n\n```markdown\n---\nsources:\n  - ./fake.md\n---\n```\n";
        let links = parse(content);
        let fm: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == "frontmatter")
            .collect();
        assert!(
            fm.is_empty(),
            "frontmatter inside code block should be ignored"
        );
    }

    #[test]
    fn type_filter_works() {
        let parser = MarkdownParser {
            file_filter: None,
            type_filter: Some(vec!["frontmatter".to_string()]),
            extract_metadata: false,
        };
        let content = "---\nsources:\n  - ./ref.md\n---\n\n[inline](other.md) and [[wikilink]]\n";
        let links = parser.parse("test.md", content).links;
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "frontmatter");
    }

    #[test]
    fn no_filter_matches_everything() {
        let parser = MarkdownParser {
            file_filter: None,
            type_filter: None,
            extract_metadata: false,
        };
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
            type_filter: None,
            extract_metadata: false,
        };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("page.mdx"));
        assert!(!parser.matches("main.rs"));
    }

    fn parse_with_metadata(content: &str) -> super::ParseResult {
        let parser = MarkdownParser {
            file_filter: None,
            type_filter: None,
            extract_metadata: true,
        };
        parser.parse("test.md", content)
    }

    #[test]
    fn extracts_frontmatter_metadata() {
        let content = "---\ntitle: My Doc\nstatus: draft\ntags:\n  - rust\n  - cli\n---\n\n# Hello\n";
        let result = parse_with_metadata(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["title"], "My Doc");
        assert_eq!(meta["status"], "draft");
        assert_eq!(meta["tags"], serde_json::json!(["rust", "cli"]));
    }

    #[test]
    fn no_metadata_without_frontmatter() {
        let result = parse_with_metadata("# Just a heading\n");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn no_metadata_when_disabled() {
        let parser = MarkdownParser {
            file_filter: None,
            type_filter: None,
            extract_metadata: false,
        };
        let content = "---\ntitle: My Doc\n---\n\n# Hello\n";
        let result = parser.parse("test.md", content);
        assert!(result.metadata.is_none());
    }

    #[test]
    fn metadata_ignores_code_block_frontmatter() {
        let content = "# Doc\n\n```markdown\n---\ntitle: Fake\n---\n```\n";
        let result = parse_with_metadata(content);
        assert!(result.metadata.is_none());
    }

    #[test]
    fn metadata_handles_nested_yaml() {
        let content = "---\ntitle: Test\nauthor:\n  name: Alice\n  role: dev\n---\n";
        let result = parse_with_metadata(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["author"]["name"], "Alice");
        assert_eq!(meta["author"]["role"], "dev");
    }
}
