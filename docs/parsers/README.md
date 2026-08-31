---
purpose: how parsers extract links and metadata from a graph's files
sources:
  - ../../src/parsers/mod.rs
---

# Parsers

A parser interprets the files a graph's `files` globs select, emitting links that
become edges. Each parser also emits node metadata about the file it read: the
markdown parser the `#fragment` anchors that file answers to, the frontmatter
parser the parsed frontmatter block. A graph names its parser with
`parser = "..."`.

| Parser                        | Emits                                                        |
| ----------------------------- | ------------------------------------------------------------ |
| [markdown](markdown.md)       | edges (inline, reference, autolink, image) + heading anchors |
| [frontmatter](frontmatter.md) | edges (values under declared keys) + node metadata           |
