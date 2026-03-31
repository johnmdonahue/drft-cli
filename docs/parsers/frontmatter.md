# Frontmatter parser

## The concept

The frontmatter parser extracts YAML frontmatter from files. It serves two purposes: detecting file path references as edges, and extracting structured metadata for rules like schema-violation.

## Link types

The parser extracts one type of link. Each becomes an edge with parser provenance `frontmatter` in the graph.

### link

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

## Metadata

The parser always extracts the full YAML frontmatter as structured metadata, attached to the node as `node.metadata["frontmatter"]`. This makes frontmatter fields available to rules like schema-violation.

## Configuration

### Minimal

```toml
[parsers.frontmatter]
```

With no `files` field, the parser receives all File nodes in the graph.

### File routing

Restrict which File nodes the parser receives:

```toml
[parsers.frontmatter]
files = ["*.md"]
```

### With schema validation

```toml
[parsers.frontmatter]
files = ["*.md"]

[rules.schema-violation]
severity = "warn"

[rules.schema-violation.options]
required = ["title"]

[rules.schema-violation.options.schemas."observations/*.md"]
required = ["title", "date", "status"]
allowed.status = ["draft", "review", "final"]
```

## Source

[`src/parsers/frontmatter.rs`](../../src/parsers/frontmatter.rs)
