# parsers

Parsers extract links from files. Each implements the `Parser` trait.

- [markdown.rs](markdown.rs) — built-in markdown parser (inline links, frontmatter, wikilinks, images, references)
- [mod.rs](mod.rs) — `Parser` trait, `RawLink` type, parser registry
- [script.rs](script.rs) — script-based parser runner (batch NDJSON protocol)
