use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};

use super::{Link, ParseResult, Parser};

/// Check whether a frontmatter value looks like a link target (file path or URI).
fn is_link_candidate(value: &str) -> bool {
    // URIs are always candidates — graph builder classifies them as External(Remote)
    if crate::util::is_uri(value) {
        return true;
    }
    // Explicit path prefixes are always candidates.
    // The graph builder gates all filesystem access for out-of-root targets.
    if value.starts_with("./") || value.starts_with("../") || value.starts_with('/') {
        return true;
    }
    // Prose contains spaces — file paths don't
    if value.contains(' ') {
        return false;
    }
    // Must have a plausible file extension: dot followed by 1-6 alphanumeric
    // chars that aren't all digits (rejects v2.0, e.g., Dr.)
    let basename = value.rsplit('/').next().unwrap_or(value);
    if let Some(dot_pos) = basename.rfind('.') {
        let ext = &basename[dot_pos + 1..];
        !ext.is_empty()
            && ext.len() <= 6
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && !ext.chars().all(|c| c.is_ascii_digit())
    } else {
        false
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

/// Built-in frontmatter parser. Extracts YAML frontmatter as links and metadata.
pub struct FrontmatterParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
    /// Keys whose values yield edges. `None` falls back to shape detection over
    /// the whole block, which cannot tell a derivation (`sources:`) from a value
    /// that merely looks like a path (`route: /customers`). Naming the keys lets
    /// the config say what the graph tracks. Scopes edges only — metadata always
    /// captures the entire block.
    pub keys: Option<Vec<String>>,
}

impl Parser for FrontmatterParser {
    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true,
        }
    }

    fn parse(&self, _path: &str, content: &str) -> ParseResult {
        // `stripped` is owned and outlives the borrowed marked AST below.
        let stripped = strip_code(content);
        let Some(yaml_str) = frontmatter_block(&stripped) else {
            return ParseResult::default();
        };
        // Malformed YAML contributes nothing — drft is not a YAML linter.
        let Ok(docs) = MarkedYaml::load_from_str(yaml_str) else {
            return ParseResult::default();
        };
        let Some(root) = docs.first() else {
            return ParseResult::default();
        };

        let mut candidates = Vec::new();
        match &self.keys {
            Some(keys) => {
                let wanted: std::collections::HashSet<&str> =
                    keys.iter().map(String::as_str).collect();
                collect_scoped(root, &wanted, &mut candidates);
            }
            None => collect_links(root, &mut candidates),
        }
        let links = candidates
            .into_iter()
            .filter(|(value, _)| is_link_candidate(value))
            .map(|(target, line)| Link {
                target,
                line: Some(line),
            })
            .collect();

        ParseResult {
            links,
            metadata: Some(to_json(root)),
        }
    }
}

/// Extract the YAML frontmatter block from code-stripped content — the text
/// between the opening `---` and the next `\n---`, or `None` when there is no
/// well-formed block. The slice keeps the newline that follows the opening
/// fence, so a node's line within the block equals its line within the file.
fn frontmatter_block(stripped: &str) -> Option<&str> {
    let rest = stripped.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let yaml_str = &rest[..end];
    if yaml_str.trim().is_empty() {
        return None;
    }
    Some(yaml_str)
}

/// Collect string leaf *values* (not keys) with their 1-based source line — the
/// frontmatter link candidates. Mirrors the metadata walk but keeps only strings.
fn collect_links(node: &MarkedYaml, out: &mut Vec<(String, usize)>) {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => out.push((s.to_string(), node.span.start.line())),
        YamlData::Sequence(items) => {
            for item in items {
                collect_links(item, out);
            }
        }
        YamlData::Mapping(map) => {
            for (_key, value) in map {
                collect_links(value, out);
            }
        }
        YamlData::Tagged(_, inner) => collect_links(inner, out),
        _ => {}
    }
}

/// Collect scalars reachable only through one of `keys`. A matched key hands its
/// whole subtree to `collect_links`, so a nested map or list under `sources:`
/// still yields every path beneath it. Unmatched keys are still descended into,
/// so a key nested under an unrelated one is found — the key is what scopes the
/// walk, not its depth. A scalar reached under no matched key yields nothing.
fn collect_scoped(
    node: &MarkedYaml,
    keys: &std::collections::HashSet<&str>,
    out: &mut Vec<(String, usize)>,
) {
    match &node.data {
        YamlData::Sequence(items) => {
            for item in items {
                collect_scoped(item, keys, out);
            }
        }
        YamlData::Mapping(map) => {
            for (key, value) in map {
                match &key.data {
                    YamlData::Value(Scalar::String(k)) if keys.contains(k.as_ref()) => {
                        collect_links(value, out)
                    }
                    _ => collect_scoped(value, keys, out),
                }
            }
        }
        YamlData::Tagged(_, inner) => collect_scoped(inner, keys, out),
        _ => {}
    }
}

/// Convert a marked YAML node to `serde_json::Value` for the `@frontmatter`
/// metadata namespace.
fn to_json(node: &MarkedYaml) -> serde_json::Value {
    use serde_json::Value as J;
    match &node.data {
        YamlData::Value(scalar) => scalar_to_json(scalar),
        YamlData::Representation(raw, _, _) => J::String(raw.to_string()),
        YamlData::Sequence(items) => J::Array(items.iter().map(to_json).collect()),
        YamlData::Mapping(map) => {
            let obj: serde_json::Map<String, J> = map
                .iter()
                .filter_map(|(k, v)| Some((json_key(k)?, to_json(v))))
                .collect();
            J::Object(obj)
        }
        YamlData::Tagged(_, inner) => to_json(inner),
        YamlData::Alias(_) | YamlData::BadValue => J::Null,
    }
}

/// Render a mapping key as a JSON object key: a string scalar verbatim, anything
/// else as its JSON serialization (matching the prior behavior).
fn json_key(node: &MarkedYaml) -> Option<String> {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => Some(s.to_string()),
        _ => serde_json::to_string(&to_json(node)).ok(),
    }
}

fn scalar_to_json(scalar: &Scalar) -> serde_json::Value {
    use serde_json::Value as J;
    match scalar {
        Scalar::Null => J::Null,
        Scalar::Boolean(b) => J::Bool(*b),
        Scalar::Integer(i) => J::Number((*i).into()),
        Scalar::FloatingPoint(f) => serde_json::Number::from_f64(f.0)
            .map(J::Number)
            .unwrap_or(J::Null),
        Scalar::String(s) => J::String(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(content: &str) -> ParseResult {
        let parser = FrontmatterParser {
            file_filter: None,
            keys: None,
        };
        parser.parse("test.md", content)
    }

    #[test]
    fn extracts_frontmatter_links() {
        let content =
            "---\nsources:\n  - ../shared/glossary.md\n  - ./prior-art.md\n---\n\n# Hello\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "../shared/glossary.md");
        assert_eq!(result.links[1].target, "./prior-art.md");
    }

    #[test]
    fn extracts_same_directory_links() {
        let content = "---\nsources:\n  - setup.md\n  - config.rs\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "setup.md");
        assert_eq!(result.links[1].target, "config.rs");
    }

    #[test]
    fn records_link_line_numbers() {
        // Lines are 1-based and file-accurate: the opening `---` is line 1.
        let content = "---\ntitle: Doc\nsources:\n  - setup.md\n  - other.md\n---\n";
        let result = parse(content);
        let setup = result
            .links
            .iter()
            .find(|l| l.target == "setup.md")
            .unwrap();
        assert_eq!(setup.line, Some(4));
        let other = result
            .links
            .iter()
            .find(|l| l.target == "other.md")
            .unwrap();
        assert_eq!(other.line, Some(5));
    }

    #[test]
    fn malformed_yaml_contributes_nothing() {
        // Invalid YAML yields no links and no metadata — drft is not a linter,
        // and there is no stderr warning (the `eprintln` is gone).
        let result = parse("---\nsources: [a, b\n---\n");
        assert!(result.links.is_empty());
        assert!(result.metadata.is_none());
    }

    /// Parse with `keys` scoping, returning the edge targets.
    fn scoped(content: &str, keys: &[&str]) -> Vec<String> {
        let parser = FrontmatterParser {
            file_filter: None,
            keys: Some(keys.iter().map(|k| k.to_string()).collect()),
        };
        parser
            .parse("doc.md", content)
            .links
            .into_iter()
            .map(|l| l.target)
            .collect()
    }

    #[test]
    fn keys_scope_excludes_other_keys() {
        // The two real collisions from #73: an API route and a rule's glob scope,
        // both path-shaped, neither a derivation.
        let content = "---\nsources:\n  - ../src/lib.rs\nroute: /customers\npaths:\n  - \"api/openapi.yaml\"\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["../src/lib.rs"]);
    }

    #[test]
    fn keys_scope_takes_whole_subtree() {
        // A matched key hands its entire subtree over, so nesting under it still
        // yields every path beneath.
        let content = "---\nsources:\n  primary:\n    - ../a.rs\n  secondary: ../b.rs\n---\n";
        let mut got = scoped(content, &["sources"]);
        got.sort();
        assert_eq!(got, vec!["../a.rs", "../b.rs"]);
    }

    #[test]
    fn keys_scope_finds_nested_key() {
        // The key scopes the walk, not its depth — `sources` under an unrelated
        // parent is still found.
        let content = "---\nmeta:\n  sources:\n    - ../a.rs\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["../a.rs"]);
    }

    #[test]
    fn keys_scope_keeps_line_numbers() {
        let content = "---\ntitle: T\nsources:\n  - ../a.rs\n---\n";
        let parser = FrontmatterParser {
            file_filter: None,
            keys: Some(vec!["sources".to_string()]),
        };
        let links = parser.parse("doc.md", content).links;
        assert_eq!(links[0].line, Some(4));
    }

    #[test]
    fn keys_scope_still_shape_filters() {
        // Scoping picks the key; `is_link_candidate` still rejects prose under it.
        let content = "---\nsources:\n  - ../a.rs\n  - not a path at all\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["../a.rs"]);
    }

    #[test]
    fn keys_scope_leaves_metadata_whole() {
        // `keys` scopes edges only — the metadata namespace keeps the full block.
        let content = "---\ntitle: T\nroute: /customers\nsources:\n  - ../a.rs\n---\n";
        let parser = FrontmatterParser {
            file_filter: None,
            keys: Some(vec!["sources".to_string()]),
        };
        let meta = parser.parse("doc.md", content).metadata.unwrap();
        assert_eq!(meta["title"], "T");
        assert_eq!(meta["route"], "/customers");
    }

    #[test]
    fn no_keys_scope_is_shape_detection() {
        // The default is unchanged: every path-shaped value is a candidate.
        let content = "---\nroute: /customers\nsources:\n  - ../a.rs\n---\n";
        let mut got: Vec<String> = parse(content).links.into_iter().map(|l| l.target).collect();
        got.sort();
        assert_eq!(got, vec!["../a.rs", "/customers"]);
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
        let parser = FrontmatterParser {
            file_filter: None,
            keys: None,
        };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("main.rs"));
    }

    #[test]
    fn file_filter_restricts_matching() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("*.md").unwrap());
        let parser = FrontmatterParser {
            file_filter: Some(builder.build().unwrap()),
            keys: None,
        };
        assert!(parser.matches("index.md"));
        assert!(!parser.matches("main.rs"));
    }

    #[test]
    fn extracts_uris() {
        let content = "---\nsources:\n  - https://example.com\n  - ./local.md\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "https://example.com");
        assert_eq!(result.links[1].target, "./local.md");
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
        assert_eq!(result.links[0].target, "config.rs");
        assert_eq!(result.links[1].target, "docs/setup.md");
    }

    #[test]
    fn emits_absolute_paths() {
        let content = "---\nsource: /usr/local/config.toml\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "/usr/local/config.toml");
    }

    #[test]
    fn yaml_list_values_not_parsed_as_uris() {
        // Regression: `- name: foo bar bazz` was split on `- ` to get
        // `name: foo bar bazz`, which the old is_uri matched as scheme `name:`
        let content = "---\ntags:\n  - name: foo bar bazz\n  - status: draft\n---\n";
        let result = parse(content);
        assert!(result.links.is_empty());
    }
}
