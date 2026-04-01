---
sources:
  - ../../src/parsers/mod.rs
---

# Parsers

Parsers extract edges and metadata from files. Each parser receives File nodes (optionally filtered by `files` globs), parses their content, and emits links that become edges in the graph. Parsers can also emit structured metadata that is attached to nodes.

drft ships built-in parsers. Additional parsers can be added via [custom parsers](custom.md).

| Parser                        | Link types                         | Built-in? |
| ----------------------------- | ---------------------------------- | --------- |
| [frontmatter](frontmatter.md) | link                               | Yes       |
| [markdown](markdown.md)       | inline, reference, autolink, image | Yes       |
| [custom](custom.md)           | (user-defined)                     | No        |
