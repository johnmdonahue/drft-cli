# Markdown parser

## The concept

The markdown parser is drft's built-in parser for standard markdown link syntax. It scans markdown files and extracts the links that form edges in the dependency graph. For YAML frontmatter links and metadata, see the [frontmatter parser](frontmatter.md). For wikilinks (`[[page]]`), see the [example script parser](../../examples/parsers/wikilinks.sh).

## Link types

The parser extracts four types of links. Each becomes an edge with type `markdown:<type>` in the graph.

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

Images follow the same resolution rules as inline links -- fragments are stripped, anchor-only references are skipped.

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
files = ["*.md", "*.mdx"]
```

This is routing only — it does not affect which paths become nodes (that's `include`/`exclude`).

### Disable

```toml
[parsers]
markdown = false
```

## External URLs

External links (`http://` and `https://`) are emitted as raw targets by the parser. The graph builder classifies them as External nodes. Anchor-only links (`#heading`) and `mailto:` links are filtered by the graph builder. Fragment stripping (`file.md#section` → `file.md`) is also handled by the graph builder, not the parser.

## Source

[`src/parsers/markdown.rs`](../../src/parsers/markdown.rs)
