---
sources:
  - ../../src/parsers/markdown.rs
---

# Markdown parser

## The concept

The markdown parser is drft's built-in parser for standard markdown link syntax. It scans markdown files and extracts the links that form edges in the dependency graph. For YAML frontmatter links and metadata, see the [frontmatter parser](frontmatter.md).

## Link types

Each link type becomes an edge with parser provenance `markdown` in the graph.
Every edge records the 1-based source line(s) where the link appears as `lines`
in its `markdown` metadata, exposed in `drft graph` and used by `drft impact` to
point a review at the exact reference; a target linked from more than one line
carries each.

### inline

Standard markdown links where the URL is inline with the text.

```markdown
[setup guide](setup.md)
[with fragment](setup.md#installation)
```

Fragments (`#heading`) are stripped -- `setup.md#installation` produces an edge to `setup.md`.

### reference

Reference-style links, including collapsed and shortcut forms.

```markdown
[setup guide][ref]

[ref]: setup.md
```

Collapsed (`[ref][]`) and shortcut (`[ref]`) forms are also detected as reference links.

### autolink

Autolinks using angle bracket syntax.

```markdown
<https://example.com>
```

Email autolinks (`<user@example.com>`) are skipped entirely.

### image

Image references.

```markdown
![architecture diagram](assets/arch.png)
```

Images follow the same resolution rules as inline links -- the graph builder strips fragments and skips anchor-only references.

## Configuration

Declare a graph that uses the markdown parser:

```toml
[graphs.markdown]
parser = "markdown"
files = ["**/*.md", "**/*.mdx"]
```

`files` scopes which files the parser reads (default `["**/*.md"]`). There are no
default graphs — the markdown parser runs only where you declare it. See
[configuration](../config.md) for the full graph schema.

## External URLs

External links (`http://`, `https://`, `mailto:`, and other URI schemes) are emitted as raw edge targets. A URI target has no defining node, so the `unresolved-edge` rule treats it as an intentional external reference and does not flag it. Anchor-only links (`#heading`) are dropped, and fragments are stripped from targets (`file.md#section` → `file.md`) — both handled when the edge is built, not by the parser.
