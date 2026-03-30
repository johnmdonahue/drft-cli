# Markdown parser

## The concept

The markdown parser is drft's built-in parser. It scans markdown files and extracts the links that form edges in the dependency graph. It understands standard markdown link syntax, images, YAML frontmatter references, and wikilinks.

## Link types

The parser extracts six types of links. Each becomes an edge with type `markdown:<type>` in the graph.

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

### frontmatter

File path references found in YAML frontmatter.

```markdown
---
sources:
  - ../shared/glossary.md
  - ./prior-art.md
template: docs/templates/page.md
---
```

The parser uses a heuristic to distinguish file paths from plain string values: a value is treated as a path if it contains a `/` or starts with `./`, and the last path component contains a `.` (i.e., it has a file extension). Values that start with `{`, `[`, `"`, or `'` are skipped, as are URLs starting with `http://` or `https://`.

This means `sources: ../shared/glossary.md` is detected, but `title: My Document` and `version: 1.0` are not.

### wikilink

Double-bracket links in the style of wiki systems.

```markdown
See [[setup]] for details.
Link to [[guides/intro]] with paths.
Display text: [[setup|Setup Guide]].
```

`[[page]]` resolves to `page.md`. If the target already ends in `.md`, no suffix is added. The pipe syntax `[[page|display text]]` uses the portion before `|` as the target.

Wikilinks inside code (fenced blocks and inline backtick spans) are ignored. The parser strips all code content before scanning for `[[...]]` patterns, so shell syntax like `[[ $FOO == *.md ]]` in code will not produce false edges.

## Configuration

### Enable (default)

```toml
[parsers]
markdown = true
```

When `markdown = true`, the parser uses the default glob `*.md` and extracts all six link types.

### Type filtering

Restrict which link types the parser extracts:

```toml
[parsers]
markdown = ["frontmatter", "wikilink"]
```

This runs the full parser but only keeps links whose type matches the list. Useful if you only want to track certain kinds of dependencies.

### Custom glob

Override the default file matching pattern:

```toml
[parsers.markdown]
glob = "*.{md,mdx}"
```

The glob matches against filenames only (not full paths), so `*.md` matches `docs/guide.md`.

### Expanded form

Combine glob and type filter in the table form:

```toml
[parsers.markdown]
glob = "*.{md,mdx}"
types = ["inline", "image"]
```

### Disable

```toml
[parsers]
markdown = false
```

## External URLs

External links (`http://` and `https://`) are recorded in the graph with `is_external: true`. They are not skipped -- they become edges that analyses and rules can reason about. Anchor-only links (`#heading`) and `mailto:` links are skipped entirely.
