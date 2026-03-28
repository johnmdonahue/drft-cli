use crate::graph::EdgeType;
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone)]
pub struct RawLink {
    pub target: String,
    pub link_type: EdgeType,
    /// Whether this is an external URL (http/https)
    pub is_external: bool,
}

/// Extract all links from markdown content, including markdown links, frontmatter sources,
/// and wikilinks. Returns external URLs with is_external=true.
pub fn extract_links(content: &str) -> Vec<RawLink> {
    let mut links = Vec::new();

    // Extract frontmatter sources before parsing markdown
    links.extend(extract_frontmatter(content));

    // Extract wikilinks before parsing markdown (pulldown-cmark doesn't know about them)
    links.extend(extract_wikilinks(content));

    // Parse markdown links
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(content, options);

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
                if let Some(link) = process_link(&dest_url, EdgeType::Image) {
                    links.push(link);
                }
            }
            _ => {}
        }
    }

    links
}

fn map_link_type(lt: LinkType) -> EdgeType {
    match lt {
        LinkType::Inline => EdgeType::Inline,
        LinkType::Reference | LinkType::ReferenceUnknown => EdgeType::Reference,
        LinkType::Collapsed | LinkType::CollapsedUnknown => EdgeType::Reference,
        LinkType::Shortcut | LinkType::ShortcutUnknown => EdgeType::Reference,
        LinkType::Autolink | LinkType::Email => EdgeType::Autolink,
    }
}

fn process_link(url: &str, link_type: EdgeType) -> Option<RawLink> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }

    // External URLs — record as external, don't skip
    if url.starts_with("http://") || url.starts_with("https://") {
        return Some(RawLink {
            target: url.to_string(),
            link_type,
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
        link_type,
        is_external: false,
    })
}

/// Extract file path references from YAML frontmatter.
/// Paths must contain a slash or start with `./` and have a file extension.
fn extract_frontmatter(content: &str) -> Vec<RawLink> {
    let mut links = Vec::new();

    // Check for frontmatter delimiters
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

        // Extract value from "- path" or "key: path" patterns
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

        // Skip YAML inline objects/arrays and quoted strings
        if value.starts_with('{')
            || value.starts_with('[')
            || value.starts_with('"')
            || value.starts_with('\'')
        {
            continue;
        }

        // Must look like a file path: contains / or starts with ./ AND has a file extension
        let looks_like_path =
            (value.contains('/') || value.starts_with("./")) && has_file_extension(value);

        if !looks_like_path {
            continue;
        }

        // Skip URLs
        if value.starts_with("http://") || value.starts_with("https://") {
            continue;
        }

        links.push(RawLink {
            target: value.to_string(),
            link_type: EdgeType::Frontmatter,
            is_external: false,
        });
    }

    links
}

fn has_file_extension(path: &str) -> bool {
    if let Some(basename) = path.rsplit('/').next() {
        basename.contains('.')
    } else {
        path.contains('.')
    }
}

/// Extract wikilinks: [[page]] or [[page|display text]].
/// Resolves to page.md in the same directory.
fn extract_wikilinks(content: &str) -> Vec<RawLink> {
    let mut links = Vec::new();
    let mut rest = content;

    while let Some(start) = rest.find("[[") {
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("]]") {
            let inner = &after_open[..end];

            // Skip if empty or contains newlines
            if !inner.is_empty() && !inner.contains('\n') {
                // [[page|display text]] → use page
                let page = match inner.find('|') {
                    Some(idx) => &inner[..idx],
                    None => inner,
                };
                let page = page.trim();

                if !page.is_empty() {
                    // Append .md if not already present
                    let target = if page.ends_with(".md") {
                        page.to_string()
                    } else {
                        format!("{page}.md")
                    };

                    links.push(RawLink {
                        target,
                        link_type: EdgeType::Wikilink,
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

    #[test]
    fn extracts_inline_links() {
        let links = extract_links("[setup](setup.md) and [faq](faq.md)");
        let local: Vec<_> = links.iter().filter(|l| !l.is_external).collect();
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].target, "setup.md");
        assert_eq!(local[0].link_type, EdgeType::Inline);
        assert_eq!(local[1].target, "faq.md");
    }

    #[test]
    fn strips_fragment() {
        let links = extract_links("[setup](setup.md#installation)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "setup.md");
    }

    #[test]
    fn captures_external_urls() {
        let links = extract_links("[google](https://google.com) and [local](setup.md)");
        assert_eq!(links.len(), 2);
        let external: Vec<_> = links.iter().filter(|l| l.is_external).collect();
        assert_eq!(external.len(), 1);
        assert_eq!(external[0].target, "https://google.com");
        let local: Vec<_> = links.iter().filter(|l| !l.is_external).collect();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].target, "setup.md");
    }

    #[test]
    fn skips_anchor_only() {
        let links = extract_links("[section](#heading) and [local](setup.md)");
        let local: Vec<_> = links.iter().filter(|l| !l.is_external).collect();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].target, "setup.md");
    }

    #[test]
    fn skips_email_links() {
        let links = extract_links("Contact (<user@example.com>)");
        assert!(links.is_empty());
    }

    #[test]
    fn extracts_image_links() {
        let links = extract_links("![diagram](assets/arch.png)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "assets/arch.png");
        assert_eq!(links[0].link_type, EdgeType::Image);
    }

    #[test]
    fn extracts_reference_links() {
        let links = extract_links("[setup][ref]\n\n[ref]: setup.md\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "setup.md");
        assert_eq!(links[0].link_type, EdgeType::Reference);
    }

    #[test]
    fn handles_empty_and_fragment_only_after_strip() {
        let links = extract_links("[a](#only-fragment)");
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn extracts_frontmatter_sources() {
        let content =
            "---\nsources:\n  - ../shared/glossary.md\n  - ./prior-art.md\n---\n\n# Hello\n";
        let links = extract_links(content);
        let fm: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == EdgeType::Frontmatter)
            .collect();
        assert_eq!(fm.len(), 2);
        assert_eq!(fm[0].target, "../shared/glossary.md");
        assert_eq!(fm[1].target, "./prior-art.md");
    }

    #[test]
    fn frontmatter_skips_non_paths() {
        let content = "---\ntitle: My Document\nversion: 1.0\ntags:\n  - rust\n  - cli\n---\n";
        let links = extract_links(content);
        let fm: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == EdgeType::Frontmatter)
            .collect();
        assert!(fm.is_empty());
    }

    #[test]
    fn extracts_wikilinks() {
        let links = extract_links("See [[setup]] for details and [[guides/intro]].");
        let wl: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == EdgeType::Wikilink)
            .collect();
        assert_eq!(wl.len(), 2);
        assert_eq!(wl[0].target, "setup.md");
        assert_eq!(wl[1].target, "guides/intro.md");
    }

    #[test]
    fn wikilink_with_display_text() {
        let links = extract_links("See [[setup|Setup Guide]] for details.");
        let wl: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == EdgeType::Wikilink)
            .collect();
        assert_eq!(wl.len(), 1);
        assert_eq!(wl[0].target, "setup.md");
    }

    #[test]
    fn wikilink_already_has_extension() {
        let links = extract_links("See [[README.md]] here.");
        let wl: Vec<_> = links
            .iter()
            .filter(|l| l.link_type == EdgeType::Wikilink)
            .collect();
        assert_eq!(wl.len(), 1);
        assert_eq!(wl[0].target, "README.md");
    }
}
