//! Text parsers: the link/metadata extractors the text builders wrap.
//! `markdown` extracts body links; `frontmatter` extracts YAML frontmatter links
//! and the metadata block.

pub mod frontmatter;
pub mod markdown;

/// Combined output from parsing a single file: links + optional metadata.
/// Links are raw strings as they appear in the source — the builder handles
/// normalization (fragment stripping, anchor filtering, URI detection).
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub links: Vec<String>,
    /// Structured metadata extracted from the file.
    pub metadata: Option<serde_json::Value>,
}

/// Trait implemented by the built-in text parsers.
pub trait Parser {
    /// Check whether this parser should run on a given file path.
    fn matches(&self, path: &str) -> bool;
    /// Parse a file's content and return discovered links + optional metadata.
    fn parse(&self, path: &str, content: &str) -> ParseResult;
}
