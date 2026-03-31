# parsers

Parsers extract edges and metadata from files. Each implements the `Parser` trait, returning a `ParseResult` (links + optional metadata).

- [markdown.rs](markdown.rs) — built-in markdown parser (inline links, frontmatter, wikilinks, images, references, metadata extraction)
- [mod.rs](mod.rs) — `Parser` trait, `RawLink` type, `ParseResult` type, parser registry
- [script.rs](script.rs) — script-based parser runner (batch NDJSON protocol with edge and metadata lines)
