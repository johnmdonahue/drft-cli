use super::{ParseResult, Parser, is_link_candidate, strip_code};

/// Built-in frontmatter parser. Extracts YAML frontmatter as links and metadata.
pub struct FrontmatterParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
}

impl Parser for FrontmatterParser {
    fn name(&self) -> &str {
        "frontmatter"
    }

    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true,
        }
    }

    fn parse(&self, _path: &str, content: &str) -> ParseResult {
        let links = extract_frontmatter_links(content);
        let metadata = extract_frontmatter_metadata(content);

        ParseResult { links, metadata }
    }
}

/// Extract file path references from YAML frontmatter.
/// Operates on code-block-stripped content to avoid parsing frontmatter
/// inside fenced code block examples.
fn extract_frontmatter_links(content: &str) -> Vec<String> {
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

        // Skip numeric values (e.g., "1.0") — not file paths
        if value.parse::<f64>().is_ok() {
            continue;
        }

        if !is_link_candidate(value) {
            continue;
        }

        links.push(value.to_string());
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

    match serde_yml::from_str::<serde_yml::Value>(yaml_str) {
        Ok(yaml_val) => Some(yaml_to_json(yaml_val)),
        Err(e) => {
            eprintln!("warn: frontmatter parser: invalid YAML: {e}");
            None
        }
    }
}

/// Convert serde_yml::Value to serde_json::Value.
fn yaml_to_json(yaml: serde_yml::Value) -> serde_json::Value {
    match yaml {
        serde_yml::Value::Null => serde_json::Value::Null,
        serde_yml::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yml::Value::Number(n) => {
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
        serde_yml::Value::String(s) => serde_json::Value::String(s),
        serde_yml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.into_iter().map(yaml_to_json).collect())
        }
        serde_yml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        serde_yml::Value::String(s) => s,
                        other => serde_json::to_string(&yaml_to_json(other)).ok()?,
                    };
                    Some((key, yaml_to_json(v)))
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yml::Value::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> ParseResult {
        let parser = FrontmatterParser { file_filter: None };
        parser.parse("test.md", content)
    }

    #[test]
    fn parser_name() {
        let parser = FrontmatterParser { file_filter: None };
        assert_eq!(parser.name(), "frontmatter");
    }

    #[test]
    fn extracts_frontmatter_links() {
        let content =
            "---\nsources:\n  - ../shared/glossary.md\n  - ./prior-art.md\n---\n\n# Hello\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0], "../shared/glossary.md");
        assert_eq!(result.links[1], "./prior-art.md");
    }

    #[test]
    fn extracts_same_directory_links() {
        let content = "---\nsources:\n  - setup.md\n  - config.rs\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0], "setup.md");
        assert_eq!(result.links[1], "config.rs");
    }

    #[test]
    fn frontmatter_skips_non_paths() {
        let content = "---\ntitle: My Document\nversion: 1.0\ntags:\n  - rust\n  - cli\n---\n";
        let result = parse(content);
        assert!(result.links.is_empty());
    }

    #[test]
    fn frontmatter_skips_code_block_examples() {
        let content = "# Doc\n\n```markdown\n---\nsources:\n  - ./fake.md\n---\n```\n";
        let result = parse(content);
        assert!(
            result.links.is_empty(),
            "frontmatter inside code block should be ignored"
        );
        assert!(result.metadata.is_none());
    }

    #[test]
    fn extracts_metadata() {
        let content =
            "---\ntitle: My Doc\nstatus: draft\ntags:\n  - rust\n  - cli\n---\n\n# Hello\n";
        let result = parse(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["title"], "My Doc");
        assert_eq!(meta["status"], "draft");
        assert_eq!(meta["tags"], serde_json::json!(["rust", "cli"]));
    }

    #[test]
    fn no_metadata_without_frontmatter() {
        let result = parse("# Just a heading\n");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn metadata_handles_nested_yaml() {
        let content = "---\ntitle: Test\nauthor:\n  name: Alice\n  role: dev\n---\n";
        let result = parse(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["author"]["name"], "Alice");
        assert_eq!(meta["author"]["role"], "dev");
    }

    #[test]
    fn no_filter_matches_everything() {
        let parser = FrontmatterParser { file_filter: None };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("main.rs"));
    }

    #[test]
    fn file_filter_restricts_matching() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("*.md").unwrap());
        let parser = FrontmatterParser {
            file_filter: Some(builder.build().unwrap()),
        };
        assert!(parser.matches("index.md"));
        assert!(!parser.matches("main.rs"));
    }

    #[test]
    fn extracts_uris() {
        let content = "---\nsources:\n  - https://example.com\n  - ./local.md\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0], "https://example.com");
        assert_eq!(result.links[1], "./local.md");
    }

    #[test]
    fn skips_prose_with_spaces() {
        let content = "---\npurpose: configuration reference\nstatus: needs review\n---\n";
        let result = parse(content);
        assert!(result.links.is_empty());
    }

    #[test]
    fn skips_abbreviations_and_versions() {
        let content = "---\nnote: e.g.\nversion: v2.0\nauthor: Dr.\n---\n";
        let result = parse(content);
        assert!(result.links.is_empty());
    }

    #[test]
    fn accepts_paths_without_prefix() {
        let content = "---\nsources:\n  - config.rs\n  - docs/setup.md\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0], "config.rs");
        assert_eq!(result.links[1], "docs/setup.md");
    }

    #[test]
    fn accepts_absolute_paths() {
        let content = "---\nsource: /usr/local/config.toml\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0], "/usr/local/config.toml");
    }
}
