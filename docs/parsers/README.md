---
sources:
  - ../../src/parsers/mod.rs
---

# Parsers

Parsers are the text builders. Each reads the files a graph's `filter` selects,
parses their content, and emits links that become edges. The frontmatter parser
also emits the parsed frontmatter block as metadata on the file's node.

| Parser                        | Emits                                      |
| ----------------------------- | ------------------------------------------ |
| [markdown](markdown.md)       | edges (inline, reference, autolink, image) |
| [frontmatter](frontmatter.md) | edges (link-target values) + node metadata |
