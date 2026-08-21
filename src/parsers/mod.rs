//! Text parsers: the link/metadata extractors the text builders wrap.
//! `markdown` extracts body links; `frontmatter` extracts YAML frontmatter links
//! and the metadata block.

pub mod frontmatter;
pub mod markdown;
pub mod slug;

/// One link discovered by a parser: its raw target string as it appears in the
/// source, plus the 1-based source line where it occurs (`None` when the parser
/// cannot locate it). The builder handles normalization (fragment stripping,
/// anchor filtering, URI detection) and aggregates lines onto the edge.
#[derive(Debug, Clone)]
pub struct Link {
    pub target: String,
    pub line: Option<usize>,
}

/// Combined output from parsing a single file: links, the anchors it defines,
/// and optional metadata.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub links: Vec<Link>,
    /// The `#fragment` addresses this file answers to, in document order. Empty
    /// for a parser that defines no addressable sub-file positions.
    pub anchors: Vec<String>,
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
