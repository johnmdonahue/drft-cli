---
sources:
  - ../../src/parsers/markdown.rs
---

# Markdown parser

## The concept

The markdown parser is drft's built-in parser for standard markdown link syntax. It scans markdown files and extracts the links that form edges in the dependency graph. For YAML frontmatter links and metadata, see the [frontmatter parser](frontmatter.md).

## Link types

Each link type becomes an edge with parser provenance `markdown` in the graph.

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

### Minimal

```toml
[parsers.markdown]
```

With no `files` field, the parser receives all File nodes in the graph.

### File routing

Restrict which File nodes the parser receives:

```toml
[parsers.markdown]
files = ["**/*.md", "**/*.mdx"]
```

This is routing only — it does not affect which paths become nodes (that's `include`/`exclude`).

### Disable

```toml
[parsers]
markdown = false
```

## External URLs

External links (`http://`, `https://`, `mailto:`, and other URI schemes) are emitted as raw targets by the parser. The graph builder creates referenced nodes with `type: "uri"` for them. Anchor-only links (`#heading`) are filtered by the graph builder. Fragment stripping (`file.md#section` → `file.md`) is also handled by the graph builder, not the parser.
