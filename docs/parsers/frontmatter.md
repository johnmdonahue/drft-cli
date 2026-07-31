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

### Path resolution

Paths resolve relative to the **declaring file**, the same way that file's markdown links do. From a doc at `docs/taxonomy.md`:

```yaml
sources:
  - ../src/lib.rs # → src/lib.rs
  - ./notes.md # → docs/notes.md
  - api/openapi.yaml # → docs/api/openapi.yaml
```

The last one catches people out: a path written against the graph root resolves under `docs/` and fails. Because the reported target is a path nobody wrote, the finding reads as a typo rather than a wrong base, so `unresolved-edge` names the cause when the literal text would resolve from the root:

```
warn[unresolved-edge]: docs/taxonomy.md:3 → docs/predicated/artifact/src/lib.rs (no defining node)
  hint: `predicated/artifact/src/lib.rs` resolves from the graph root, but paths resolve
        relative to the declaring file (did you mean `../predicated/artifact/src/lib.rs`?)
```

The hint is withheld for paths written `./`, `../`, or `/` — those are relative by intent, so a same-named file at the root is a coincidence rather than the mistake.

Frontmatter that is not well-formed YAML contributes no edges or metadata. drft detects link drift, not YAML validity, so it stays silent on malformed frontmatter rather than reporting it.

### Scoping to keys

Shape detection classifies by value, so any path-shaped value becomes an edge whatever key it sits under. An API route (`route: /customers`) and a glob naming the files a rule governs (`paths: ["api/**"]`) both look like paths and neither is a derivation. Declare `keys` to name the keys that yield edges:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
keys = ["sources"]
```

Only values reachable through one of those keys become edges. A matched key hands over its whole subtree, so lists and nested maps under `sources:` still yield every path beneath them, and the key is matched at any depth — `meta.sources` is found too. Values under every other key are left alone.

Scoping picks the key; the shape heuristic above still applies within it, so prose under `sources:` is still rejected. `keys` scopes edges only — [metadata](#metadata) always captures the whole block.

Omitting `keys` keeps shape detection over the entire block. Prefer `keys` where the frontmatter carries anything besides derivations: it fixes the false edges without suppressing `unresolved-edge`, which is the only finding that reports a typo'd source. A rule-level `ignore` would silence both.

## Metadata

The parser attaches the parsed frontmatter block to the file's node. In the composed graph it nests under the graph's `@<name>` namespace — `@frontmatter` for a graph named `frontmatter` — alongside the file's `@fs` facts.

## Configuration

Declare a graph that uses the frontmatter parser:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
keys = ["sources"] # optional
```

`files` scopes which files the parser reads (default `["**/*.md"]`); `keys` scopes which frontmatter keys yield edges (see [scoping to keys](#scoping-to-keys)). See [configuration](../config.md) for the full graph schema.
