//! Path, URI, hashing, and text-rendering utilities shared by the sources,
//! builders, rules, and the text output paths.

use std::path::{Component, Path};

/// Hash content with BLAKE3, returning `b3:<hex>`.
pub fn hash_bytes(content: &[u8]) -> String {
    format!("b3:{}", blake3::hash(content).to_hex())
}

/// Render a string safely on one line of text output.
///
/// A target comes from a file, so it can carry anything a YAML scalar can hold —
/// a newline from a `|` block scalar, a tab, a control character. Text output is
/// line-oriented and arrow-delimited, so an unescaped newline puts a second line
/// into `drft check` with no severity prefix, which any log parser or grep reads
/// as a different kind of record. JSON carries the real string; this is for the
/// human- and shell-facing rendering only.
pub fn one_line(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Check whether a target string is a URI.
///
/// Uses the `url` crate (WHATWG URL Standard) for parsing, then filters to URIs
/// that either have authority (`://`) or use a known opaque scheme. Without this
/// filter, any `word:stuff` parses as a URL — e.g. a YAML value like `name: foo`
/// would be treated as a URI with scheme `name`.
pub fn is_uri(target: &str) -> bool {
    match url::Url::parse(target) {
        Ok(url) => {
            if url.has_authority() {
                return true;
            }
            matches!(
                url.scheme(),
                "mailto" | "tel" | "data" | "urn" | "javascript"
            )
        }
        Err(_) => false,
    }
}

/// Normalize a relative path by resolving `.` and `..` components using path
/// APIs. Does not touch the filesystem. Always returns forward-slash separated
/// paths. Preserves leading `..` that escape above the root, and preserves an
/// absolute root/prefix (`/foo`, `C:\foo`) verbatim — both indicate graph
/// escape and must not be silently rewritten into in-graph relative paths.
pub fn normalize_relative_path(path: &str) -> String {
    let mut prefix = String::new();
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Prefix(p) => prefix.push_str(&p.as_os_str().to_string_lossy()),
            Component::RootDir => prefix.push('/'),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            Component::Normal(c) => parts.push(c.to_string_lossy().to_string()),
        }
    }
    format!("{prefix}{}", parts.join("/"))
}

/// Rewrite a root-relative `target` as a link relative to `source_file` — the
/// inverse of [`resolve_link`], for suggesting the path an author meant to
/// write. `resolve_link(source, relative_from(source, target)) == target`.
pub fn relative_from(source_file: &str, target: &str) -> String {
    let source_dir: Vec<&str> = source_file
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .split_last()
        .map(|(_, dir)| dir.to_vec())
        .unwrap_or_default();
    let target_parts: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();

    let shared = source_dir
        .iter()
        .zip(&target_parts)
        .take_while(|(a, b)| a == b)
        .count();

    let mut parts: Vec<String> = vec!["..".to_string(); source_dir.len() - shared];
    parts.extend(target_parts[shared..].iter().map(|s| s.to_string()));
    // A target inside the source's own directory needs `./` to stay a path.
    if parts.first().is_some_and(|p| p != "..") {
        return format!("./{}", parts.join("/"));
    }
    parts.join("/")
}

/// Resolve a link target relative to a source file, producing a path relative to
/// the graph root.
pub fn resolve_link(source_file: &str, raw_target: &str) -> String {
    let source_path = Path::new(source_file);
    let source_dir = source_path.parent().unwrap_or(Path::new(""));
    let joined = source_dir.join(raw_target);
    normalize_relative_path(&joined.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_simple() {
        assert_eq!(normalize_relative_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn normalize_dot_and_dotdot() {
        assert_eq!(normalize_relative_path("./a/./b"), "a/b");
        assert_eq!(normalize_relative_path("a/b/../c"), "a/c");
    }

    #[test]
    fn relative_from_walks_up_to_shared_root() {
        assert_eq!(
            relative_from("docs/taxonomy.md", "predicated/artifact/src/lib.rs"),
            "../predicated/artifact/src/lib.rs"
        );
        assert_eq!(relative_from("a/b/c.md", "a/d.md"), "../d.md");
        assert_eq!(relative_from("a/b/c.md", "x.md"), "../../x.md");
    }

    #[test]
    fn relative_from_same_directory_keeps_dot_prefix() {
        assert_eq!(relative_from("docs/a.md", "docs/b.md"), "./b.md");
        assert_eq!(relative_from("a.md", "b.md"), "./b.md");
        assert_eq!(relative_from("docs/a.md", "docs/sub/b.md"), "./sub/b.md");
    }

    #[test]
    fn relative_from_round_trips_through_resolve_link() {
        // The suggestion must actually resolve to the target it names.
        for (source, target) in [
            ("docs/taxonomy.md", "predicated/artifact/src/lib.rs"),
            ("docs/a.md", "docs/b.md"),
            ("a/b/c.md", "x.md"),
            ("README.md", "src/lib.rs"),
            ("a/b/c.md", "a/b/d/e.md"),
        ] {
            let suggestion = relative_from(source, target);
            assert_eq!(
                resolve_link(source, &suggestion),
                target,
                "{source} + {suggestion} should resolve to {target}"
            );
        }
    }

    #[test]
    fn normalize_preserves_leading_dotdot() {
        assert_eq!(normalize_relative_path("../a"), "../a");
        assert_eq!(normalize_relative_path("../../a"), "../../a");
        assert_eq!(
            normalize_relative_path("guides/../../README.md"),
            "../README.md"
        );
    }

    #[test]
    fn normalize_preserves_absolute_root() {
        // An absolute target must stay absolute, not collapse into an in-graph
        // relative path that could falsely resolve against the fs graph.
        assert_eq!(normalize_relative_path("/docs/x.md"), "/docs/x.md");
        assert_eq!(normalize_relative_path("/a/../b"), "/b");
    }

    #[test]
    fn resolve_relative_to_source() {
        assert_eq!(resolve_link("index.md", "setup.md"), "setup.md");
        assert_eq!(
            resolve_link("guides/intro.md", "setup.md"),
            "guides/setup.md"
        );
        assert_eq!(resolve_link("guides/intro.md", "../config.md"), "config.md");
    }

    #[test]
    fn is_uri_detects_schemes() {
        assert!(is_uri("http://example.com"));
        assert!(is_uri("https://example.com"));
        assert!(is_uri("mailto:user@example.com"));
        assert!(is_uri("tel:+1234567890"));
        assert!(is_uri("ssh://git@github.com"));
    }

    #[test]
    fn is_uri_rejects_paths_and_bare_schemes() {
        assert!(!is_uri("setup.md"));
        assert!(!is_uri("./relative/path.md"));
        assert!(!is_uri("../parent.md"));
        assert!(!is_uri(""));
        assert!(!is_uri("path/with:colon.md"));
        assert!(!is_uri("name: foo bar bazz"));
        assert!(!is_uri("status: draft"));
    }

    #[test]
    fn hash_has_prefix() {
        assert!(hash_bytes(b"hello").starts_with("b3:"));
    }

    #[test]
    fn one_line_escapes_what_would_break_a_line_oriented_report() {
        // A target comes from a file, so it can carry anything a YAML scalar can.
        // An unescaped newline puts a second line into `check` output with no
        // severity prefix, which a log parser reads as a different record.
        assert_eq!(one_line("a\nb"), "a\\nb");
        assert_eq!(one_line("a\r\nb"), "a\\r\\nb");
        assert_eq!(one_line("a\tb"), "a\\tb");
        assert_eq!(one_line("a\u{1b}b"), "a\\u{1b}b");
        // Ordinary text, including non-ASCII, is untouched.
        assert_eq!(one_line("docs/guide.md"), "docs/guide.md");
        assert_eq!(one_line("arrow → target"), "arrow → target");
    }
}
