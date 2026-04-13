---
sources:
  - ../../src/parsers/frontmatter.rs
---

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
  - setup.md
  - ../shared/glossary.md
  - ./prior-art.md
template: docs/templates/page.md
---
```

The parser uses `serde_yml` to parse the YAML structure, then collects all string leaf values and filters them through a heuristic: a value is treated as a link if it has an explicit path prefix (`./`, `../`, `/`), is a valid URI, or has a plausible file extension (1-6 alphanumeric characters after the last dot, not all digits). Non-string types (numbers, booleans, null) are skipped by the YAML parser. Strings with spaces are rejected as prose.

This means `sources: setup.md` and `sources: ../shared/glossary.md` are detected, but `title: My Document` and `version: 1.0` are not. YAML mapping keys within lists (e.g., `- name: foo bar`) are correctly ignored — only values are examined.

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
files = ["**/*.md"]
```

### With schema validation

```toml
[parsers.frontmatter]
files = ["**/*.md"]

[rules.schema-violation]
severity = "warn"

[rules.schema-violation.options]
required = ["title"]

[rules.schema-violation.options.schemas."observations/*.md"]
required = ["title", "date", "status"]
allowed.status = ["draft", "review", "final"]
```
