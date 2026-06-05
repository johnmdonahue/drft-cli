---
sources:
  - ../../src/parsers/mod.rs
---

# Parsers

A parser interprets the files a graph's `files` globs select, emitting links that
become edges. The frontmatter parser also emits the parsed frontmatter block as
metadata on the file's node. A graph names its parser with `parser = "..."`.

| Parser                        | Emits                                      |
| ----------------------------- | ------------------------------------------ |
| [markdown](markdown.md)       | edges (inline, reference, autolink, image) |
| [frontmatter](frontmatter.md) | edges (link-target values) + node metadata |
