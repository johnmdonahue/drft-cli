---
sources:
  - ../../src/parsers/frontmatter.rs
---

# Frontmatter parser

## The concept

The frontmatter parser extracts YAML frontmatter from files. It serves two purposes: detecting file path references as edges, and attaching the parsed frontmatter as node metadata.

## Link types

The parser extracts one type of link. Each becomes an edge with parser provenance `frontmatter` in the graph. Every edge records the 1-based source line(s) where the value appears as `lines` in its `frontmatter` metadata, exposed in `drft graph` and used by `drft impact` to point a review at the exact reference.

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

The parser parses the YAML frontmatter block, then collects all string leaf values and filters them through a heuristic: a value is treated as a link if it has an explicit path prefix (`./`, `../`, `/`), is a valid URI, or has a plausible file extension (1-6 alphanumeric characters after the last dot, not all digits). Non-string types (numbers, booleans, null) are skipped. Strings with spaces are rejected as prose.

This means `sources: setup.md` and `sources: ../shared/glossary.md` are detected, but `title: My Document` and `version: 1.0` are not. YAML mapping keys within lists (e.g., `- name: foo bar`) are correctly ignored — only values are examined.

Frontmatter that is not well-formed YAML contributes no edges or metadata. drft detects link drift, not YAML validity, so it stays silent on malformed frontmatter rather than reporting it.

## Metadata

The parser attaches the parsed frontmatter block to the file's node. In the composed graph it nests under the graph's `@<name>` namespace — `@frontmatter` for a graph named `frontmatter` — alongside the file's `@fs` facts.

## Configuration

Declare a graph that uses the frontmatter parser:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
```

`files` scopes which files the parser reads (default `["**/*.md"]`). See [configuration](../config.md) for the full graph schema.
