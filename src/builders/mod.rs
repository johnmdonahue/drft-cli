//! Builders turn a source's `(path, bytes)` records into graph nodes and edges.
//! v0.8 ships the `fs` node builder; the `markdown` and `frontmatter` text
//! builders (parsers) arrive with multi-graph compose.

pub mod fs;
