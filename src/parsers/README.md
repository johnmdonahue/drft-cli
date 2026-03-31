# parsers

Parsers extract edges and metadata from files. Each implements the `Parser` trait, returning a `ParseResult` (links + optional metadata).

- [frontmatter.rs](frontmatter.rs) — built-in frontmatter parser (YAML frontmatter link extraction + metadata)
- [markdown.rs](markdown.rs) — built-in markdown parser (inline links, images, references, autolinks)
- [mod.rs](mod.rs) — `Parser` trait, `ParseResult` type, shared utilities (`strip_code`, `has_file_extension`), parser registry
- [script.rs](script.rs) — script-based parser runner (batch NDJSON protocol with edge and metadata lines)
