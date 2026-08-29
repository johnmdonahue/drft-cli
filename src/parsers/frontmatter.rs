use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
use std::borrow::Cow;

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

/// A code-masked copy of a block, with the source line each masked line began on.
///
/// The mask blanks a code span to spaces, newlines included, because fusing a
/// span's lines is part of what lets a block parse: a span can hide a `:` that
/// would otherwise break the mapping. That fusing is also why a masked line
/// number is not a source line number — the copy is shorter by one line per
/// newline swallowed, so every value below a span resolved that many lines too
/// high.
///
/// Recording the correspondence fixes the line without touching the text. The
/// alternative — keeping the newlines so the copy stays the same shape — changes
/// what the mask *says*, not just where it says it: `collect_links` reads a
/// scalar's value out of this copy, so a span inside a link value would fold to a
/// different target string, and the edge, the lockfile entry, and the resolution
/// would move with it.
struct Masked {
    text: String,
    /// `lines[n]` is the 1-based source line that masked line `n + 1` begins on.
    lines: Vec<usize>,
}

impl Masked {
    /// The source line a value reported at 1-based `masked` line came from.
    ///
    /// A masked line fusing several source lines reports the first of them, which
    /// is where a value corrupted by that fusing begins. Splitting further would
    /// mean tracking a source line per column, and the values it would separate
    /// are the ones the mask has already altered.
    fn source_line(&self, masked: usize) -> usize {
        self.lines
            .get(masked.wrapping_sub(1))
            .copied()
            .unwrap_or(masked)
    }
}

/// Mask fenced blocks and inline backtick spans, replacing every character with a
/// space, and record where each masked line started in the source.
fn strip_code(content: &str) -> Masked {
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

    // Then strip inline code spans (single and double backticks), recording the
    // source line each masked line begins on. Only this pass fuses lines; the
    // fenced pass above rebuilds the text line for line.
    let mut cleaned = String::with_capacity(result.len());
    let mut lines = vec![1usize];
    let mut source_line = 1usize;
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
                // Blank the entire span — backticks, content, and closing
                // backticks. Under `Keep` its newlines survive: a span crossing a
                // line boundary that came back as spaces shortened the block by a
                // line, so every node below it resolved one line too high per
                // newline swallowed, and that number reaches `drft edges`, `drft
                // impact`, and every finding.
                // A blanked newline advances the source line without ending the
                // masked one — which is exactly the shift the table records.
                let total = close_start + ticks - i;
                for c in &chars[i..i + total] {
                    if *c == '\n' {
                        source_line += 1;
                    }
                    cleaned.push(' ');
                }
                i += total;
            } else {
                // No closing — keep the backtick as-is
                cleaned.push(chars[i]);
                i += 1;
            }
        } else {
            if chars[i] == '\n' {
                source_line += 1;
                lines.push(source_line);
            }
            cleaned.push(chars[i]);
            i += 1;
        }
    }

    Masked {
        text: cleaned,
        lines,
    }
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
        // Two independent parses over one block, one per job. The edge scan runs on a *masked*
        // copy — `strip_code` blanks code spans so a `path.md` written in prose
        // can't be mistaken for a link target. Metadata runs on the *raw*
        // frontmatter, so a code span in a value survives as the prose it is.
        // Both read the same block — one boundary, decided by `parsed_block` —
        // and differ only in whether spans are blanked within it.
        ParseResult {
            links: self.extract_links(content),
            // Frontmatter defines no addressable sub-file positions; anchors come
            // from the markdown body's headings.
            anchors: Vec::new(),
            metadata: extract_metadata(content),
        }
    }
}

impl FrontmatterParser {
    /// Extract frontmatter link edges from the *masked* frontmatter block, where
    /// code spans are blanked so a `path.md` written in prose is never a link
    /// target. An absent or malformed block yields no links.
    fn extract_links(&self, content: &str) -> Vec<Link> {
        // The boundary is the shared one; only the masking is this job's own.
        // Finding the block in a masked copy of the *whole file* let a code span
        // crossing the closing fence move the boundary — lifting a link out of
        // body prose in one direction, and silently dropping every declared
        // `sources:` edge in the other, on a file whose metadata still reported
        // them.
        let Some((_, block)) = parsed_block(content) else {
            return Vec::new();
        };
        // One mask, the same one the gate and the metadata fallback use, so a
        // value's text here is the text they saw. Only the reported line is
        // corrected, against the table the mask built while fusing.
        let masked = strip_code(block);
        // Malformed YAML contributes nothing — drft is not a YAML linter.
        let Ok(docs) = MarkedYaml::load_from_str(masked.text.as_str()) else {
            return Vec::new();
        };
        let Some(root) = docs.first() else {
            return Vec::new();
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
        candidates
            .into_iter()
            .filter(|(value, _)| is_link_candidate(value))
            .map(|(target, line)| Link {
                target,
                line: Some(masked.source_line(line)),
            })
            .collect()
    }
}

/// Capture the frontmatter block as node metadata. Prefers the *raw* block so a
/// code span in a value — the service name or path an author put in backticks —
/// survives as the prose it is, instead of coming back blanked.
///
/// Falls back to the masked block only when the raw block is not valid YAML on its
/// own: an unquoted value that *begins* with a backtick, or hides a `:` inside a
/// span, is rejected by every YAML parser (backtick is a reserved indicator), so
/// drft — not a YAML linter — cannot recover it. The fallback keeps the sibling
/// fields structured at the cost of blanking that one span; the author's fix is to
/// quote the value or use a `|` block scalar, both of which the raw path captures
/// verbatim. Returns `None` when there is no frontmatter block at all.
///
/// The masking applies to the block, not to the file — see [`parsed_block`].
fn extract_metadata(content: &str) -> Option<serde_json::Value> {
    let (_, block) = parsed_block(content)?;
    for candidate in [Cow::Borrowed(block), Cow::Owned(strip_code(block).text)] {
        if let Ok(docs) = MarkedYaml::load_from_str(candidate.as_ref())
            && let Some(root) = docs.first()
            // `parsed_block` already gated on one of these parsing as a mapping,
            // but not necessarily *this* one. Checking here keeps the return type
            // honest without resting on an argument about what blanking can do.
            && matches!(root.data, YamlData::Mapping(_))
        {
            return Some(to_json(root));
        }
    }
    None
}

/// The frontmatter block this parser recognizes: where it ends in `content`, and
/// its raw text.
///
/// One selector, so the offset the markdown parser masks and the metadata this
/// parser contributes can never describe different spans. They did: this looked
/// for its block in a copy of the **whole file** with code spans blanked, so a
/// backtick opened in frontmatter and closed in the body blanked the closing
/// fence and moved the boundary past it. The mask, working from the raw content,
/// saw no frontmatter at all — and the file published anchors slugged from a
/// block this parser had already claimed.
///
/// Masking is applied to the block text instead, which serves the reason it
/// exists — a code span can hide a `:` that would otherwise break the mapping —
/// without letting a span decide where the block ends.
fn parsed_block(content: &str) -> Option<(usize, &str)> {
    let block = frontmatter_block(content)?;
    let end = "---".len() + block.len() + "\n---".len();
    // Raw first, so a code span survives as the prose it is; then the same block
    // with spans blanked, since one can hide a `:` that breaks the mapping.
    let parses = is_mapping(block) || is_mapping(&strip_code(block).text);
    parses.then_some((end, block))
}

/// Whether `block` parses as a YAML **mapping** — the shape that separates
/// frontmatter from a document opening with a `---` thematic break.
///
/// A mapping is required rather than any valid YAML because YAML and markdown
/// collide on two constructs, and accepting either would delete real content:
///
/// - A comment-only block parses to no document at all. But `# First` is a YAML
///   comment *and* a markdown ATX heading, so treating one as frontmatter reads
///   `---\n\n# First\n\n---` as a block and deletes the most ordinary heading in
///   markdown, along with any link beneath it.
/// - A block parsing to a bare scalar is ambiguous the same way:
///   `---\nMy Title\n---` is equally a rule above a setext heading.
///
/// Both are left as content. The cost is that comment-only frontmatter has its
/// text read as prose; the alternative deletes a document's headings and edges,
/// and only one of those is recoverable by reading the page.
fn is_mapping(block: &str) -> bool {
    MarkedYaml::load_from_str(block)
        .ok()
        .and_then(|docs| {
            docs.first()
                .map(|doc| matches!(doc.data, YamlData::Mapping(_)))
        })
        .unwrap_or(false)
}

/// The byte offset just past a leading YAML frontmatter block, or `None` when the
/// file does not open with one.
///
/// The block has to parse as a **mapping**, which is what separates frontmatter
/// from a document that merely opens with a `---` thematic break. Only a mapping
/// contributes node metadata (see [`crate::builders::frontmatter`]), so this is
/// the same block this parser consumes — and the markdown parser masks exactly
/// what this one claims, rather than guessing at the boundary a second time and
/// disagreeing.
pub fn mapping_block_end(content: &str) -> Option<usize> {
    parsed_block(content).map(|(end, _)| end)
}

/// Extract the YAML frontmatter block — the text between the opening `---` and the
/// next `\n---`, or `None` when there is no well-formed block.
///
/// Called on the **raw** content only, through [`parsed_block`]. Calling it on a
/// `strip_code` copy of the whole file finds a different boundary whenever a span
/// crosses the closing fence, because blanking the span takes the fence with it —
/// which is how the edge scan came to read a block nothing else agreed on. The
/// slice keeps the newline after the opening fence, so a node's line within the
/// block equals its line within the file.
fn frontmatter_block(stripped: &str) -> Option<&str> {
    let rest = stripped.strip_prefix("---")?;
    // The opening fence has to be a line of its own. `---key: v` is not
    // frontmatter under any convention that writes it, and reading it as one
    // invents metadata out of a document's first paragraph.
    if !rest.starts_with('\n') && !rest.starts_with("\r\n") {
        return None;
    }
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
    fn a_code_span_crossing_the_fence_does_not_move_the_block() {
        // A backtick opened in frontmatter and closed in the body. Masking the
        // whole file would blank the closing fence and push the block past it, so
        // this parser would claim a span the markdown parser cannot see — and the
        // file would publish anchors slugged out of what this one had claimed.
        let content = "---\npurpose: `a: b\n---\nc: d` tail\nkey: v\n---\n\n# Body\n";
        assert!(parse(content).metadata.is_none());
        assert!(mapping_block_end(content).is_none(), "and the mask agrees");
    }

    #[test]
    fn an_opening_fence_must_own_its_line() {
        // `---key: v` is a paragraph, not frontmatter; reading it as one invents
        // metadata out of a document's first line.
        let result = parse("---key: v\n---\n\n# Body\n");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn mapping_block_end_agrees_with_what_this_parser_extracts() {
        // The markdown parser masks whatever this returns, so a disagreement
        // publishes an address the file does not answer to — or deletes content
        // that was never frontmatter.
        let frontmatter = "---\ntitle: Doc\n---\n\n# Body\n";
        assert_eq!(
            mapping_block_end(frontmatter),
            Some("---\ntitle: Doc\n---".len())
        );
        assert!(parse(frontmatter).metadata.is_some());

        // A code span hiding a `:`: this parser recovers via the masked copy, so
        // the boundary has to as well.
        let masked_only = "---\npurpose: use `a: b` here\n---\n\n# Body\n";
        assert!(mapping_block_end(masked_only).is_some());
        assert!(parse(masked_only).metadata.is_some());

        // A thematic break above a setext title is not frontmatter either way.
        assert!(mapping_block_end("---\nJust A Title\n---\n\nBody\n").is_none());
        assert!(mapping_block_end("---\n\n# First\n\n---\n\n# Second\n").is_none());
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
    fn metadata_keeps_code_spans_in_values() {
        // The edge-scan mask blanked code spans in captured metadata; metadata now
        // carries the raw value across every field, not just one — the issue flagged
        // `title`/`description` too, so assert a second field, not only `purpose`.
        let content = "---\npurpose: A code span like `widget-loader` and a path `claims.md`.\ntitle: the `Foo` service\n---\n";
        let meta = parse(content).metadata.unwrap();
        assert_eq!(
            meta["purpose"],
            "A code span like `widget-loader` and a path `claims.md`."
        );
        assert_eq!(meta["title"], "the `Foo` service");
    }

    #[test]
    fn a_stray_backtick_keeps_both_the_metadata_and_the_edges() {
        // An unclosed backtick in a value pairs with one in the body. Finding the
        // block in a masked copy of the whole file blanked everything between —
        // the closing `---` included — so the edge scan found no block and the
        // declared `sources:` entry produced nothing, on a file whose metadata
        // reported that same entry. The boundary comes from the raw content now,
        // so both agree.
        let content =
            "---\npurpose: a `widget\nsources:\n  - b.md\n---\n\n# Body with a `code span`\n";
        let result = parse(content);
        let meta = result.metadata.expect("metadata comes from the raw block");
        assert_eq!(meta["purpose"], "a `widget");
        assert_eq!(
            result.links.len(),
            1,
            "the declared source is still an edge"
        );
        assert_eq!(result.links[0].target, "b.md");
        assert_eq!(
            result.links[0].line,
            Some(4),
            "and its line is file-accurate"
        );
    }

    #[test]
    fn a_span_crossing_the_fence_neither_drops_nor_invents_an_edge() {
        // Direction one: a span that swallows the closing fence used to pull body
        // prose into the block, so a `deps:` line below it became a declared
        // source — with a line number off by the newlines the mask ate.
        let lifted = "---\na: `x\n---\ny`\ndeps: ./t.md\n---\n\n# Body\n";
        let result = parse(lifted);
        assert!(
            result.links.is_empty(),
            "body prose is not a declared source"
        );
        assert!(
            result.metadata.is_none(),
            "and the block is not frontmatter"
        );

        // Direction two: one backtick inside a properly quoted string, plus an
        // ordinary code span in the body, silently deleted every declared edge.
        let dropped =
            "---\nsources:\n  - ./t.md\nnote: \"has a ` backtick\"\n---\n\n# B\n\nA `code` span.\n";
        let result = parse(dropped);
        assert_eq!(result.links.len(), 1, "the declared source survives");
        assert_eq!(result.links[0].target, "./t.md");
    }

    #[test]
    fn metadata_falls_back_to_masked_when_raw_is_not_valid_yaml() {
        // A code span hiding a `:` makes the raw block invalid YAML; the masked
        // parse stands in so the block still yields structured metadata rather than
        // being dropped whole. `title` blanks to null — the tell that the *masked*
        // path ran, not raw (raw would keep the backticks or fail outright) — while
        // the sibling `status` comes through.
        let content = "---\ntitle: `key: value`\nstatus: draft\n---\n";
        let meta = parse(content)
            .metadata
            .expect("masked fallback keeps metadata when raw YAML is invalid");
        assert!(
            meta["title"].is_null(),
            "masked span blanks to null: {meta}"
        );
        assert_eq!(meta["status"], "draft");
    }

    #[test]
    fn unquoted_leading_backtick_is_invalid_yaml_and_blanks_via_fallback() {
        // Known limitation: a value that *starts* with a code span is invalid YAML
        // (backtick is a reserved indicator — Psych, saphyr, and js-yaml all reject
        // it), so the raw parse fails and the masked fallback blanks the leading
        // span. Sibling fields are unharmed. The author's fix is to quote the value
        // or use a `|` block scalar, both captured verbatim (see the tests above).
        let content = "---\npurpose: `widget-loader` is the entry point\nstatus: ok\n---\n";
        let meta = parse(content).metadata.unwrap();
        assert_eq!(meta["purpose"], "is the entry point");
        assert_eq!(meta["status"], "ok");
    }

    #[test]
    fn code_span_in_metadata_does_not_become_an_edge() {
        // Scope guard: capturing the raw value must not make a code-span path an
        // edge. The mask still governs edge extraction, so nothing links here.
        let content = "---\npurpose: see `config.rs` for details\n---\n";
        let result = parse(content);
        assert!(result.links.is_empty(), "code spans are not edges");
        assert_eq!(
            result.metadata.unwrap()["purpose"],
            "see `config.rs` for details"
        );
    }

    #[test]
    fn block_scalar_with_code_span_survives_whole() {
        // The reported case: a multi-line `purpose` block scalar whose spans were
        // blanked. Every character now comes back, newline included.
        let content = "---\npurpose: |\n  Plain words. A span like `widget-loader` stays.\nsources:\n  - b.md\n---\n";
        let result = parse(content);
        assert_eq!(
            result.metadata.unwrap()["purpose"],
            "Plain words. A span like `widget-loader` stays.\n"
        );
        // The real derivation is still an edge.
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "b.md");
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
