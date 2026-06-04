//! Path, URI, and hashing utilities shared by the sources, builders, and rules.

use std::path::Path;

/// Hash content with BLAKE3, returning `b3:<hex>`.
pub fn hash_bytes(content: &[u8]) -> String {
    format!("b3:{}", blake3::hash(content).to_hex())
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
/// paths. Preserves leading `..` that escape above the root — these indicate
/// graph escape.
pub fn normalize_relative_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            std::path::Component::Normal(c) => parts.push(c.to_string_lossy().to_string()),
            _ => {}
        }
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
    fn normalize_preserves_leading_dotdot() {
        assert_eq!(normalize_relative_path("../a"), "../a");
        assert_eq!(normalize_relative_path("../../a"), "../../a");
        assert_eq!(
            normalize_relative_path("guides/../../README.md"),
            "../README.md"
        );
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
}
