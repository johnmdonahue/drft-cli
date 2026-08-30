---
purpose: the frontmatter parser — edges from link values plus node metadata
sources:
  - ../../src/parsers/frontmatter.rs
---

# Frontmatter parser

## The concept

The frontmatter parser extracts YAML frontmatter from files. It serves two purposes: detecting file path references as edges, and attaching the parsed frontmatter as node metadata.

## Link types

The parser extracts one type of link. Each becomes an edge with parser provenance `frontmatter` in the graph. Every edge carries an `occurrences` array in its `frontmatter` metadata, one entry per value, each recording the 1-based source `line` where that value appears. `drft graph` exposes the array; `drft impact` reads the lines to point a review at the exact reference.

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

Edges and [metadata](#metadata) read the same rendering of the block. The raw block is preferred, and the masked copy — code spans blanked — is read only when the raw block is not a YAML mapping on its own. What reaches it is any block YAML rejects until a span is blanked. A value that _begins_ with a backtick is the commonest, since the character is a reserved indicator there; a span hiding a `:` is another, and a span swallowing a line break can take a `-` or a tab into a position that breaks the mapping.

Because both read the same rendering, **an edge whose value is path-shaped carries the target `@frontmatter` reports**. A code span inside a value is part of that value rather than something blanked out of it:

```yaml
sources: ./setup.md `draft`
```

That value names a target ending in `` `draft` ``, which nothing answers to, so it raises `unresolved-edge` rather than quietly resolving to `setup.md`.

A value is trimmed and has its spans blanked before the shape heuristic runs, so neither a span beside a path nor the trailing newline a `|` block scalar keeps will hide it. A span sitting _inside_ a path still can: blanking ``tar`x`get.md`` leaves a space in the middle, the heuristic reads a space as prose, and the value yields no edge. Two further qualifications, both about the direction the correspondence runs: a value the heuristic rejects has no edge to carry a target, so it holds from edges to metadata and not the other way; and a target is recorded as resolved against the declaring file, so `./setup.md` in a block appears as `setup.md` on the edge.

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
  cause: `predicated/artifact/src/lib.rs` resolves from the graph root, but paths resolve
         relative to the declaring file (did you mean `../predicated/artifact/src/lib.rs`?)
```

The `cause` is withheld for paths written `./`, `../`, or `/` — those are relative by intent, so a same-named file at the root is a coincidence rather than the mistake.

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

A block counts as frontmatter only when it parses as a YAML **mapping**, and one selector decides where it ends — for this parser's metadata, for its edges, and for the [markdown parser](markdown.md)'s mask. Code spans are blanked within that block, not used to find it: a backtick opened in frontmatter and closed in the body would otherwise move the boundary, and the two directions of that were a link lifted out of body prose and a declared `sources:` entry silently producing no edge. So a file opening with a `---` thematic break rather than frontmatter keeps its first heading and its links.

A mapping is required rather than any valid YAML because YAML and markdown collide: `# First` is a YAML comment _and_ a markdown heading, so treating a comment-only block as frontmatter would delete the most ordinary heading in markdown from any document that opens with a rule. A block parsing to a bare scalar is ambiguous the same way — `---`, `My Title`, `---` is equally a rule above a setext heading. Both are read as content, which costs comment-only frontmatter its metadata and is the recoverable direction to be wrong in.

## Configuration

Declare a graph that uses the frontmatter parser:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
keys = ["sources"] # optional
```

`files` scopes which files the parser reads (default `["**/*.md"]`); `keys` scopes which frontmatter keys yield edges (see [scoping to keys](#scoping-to-keys)). See [configuration](../config.md) for the full graph schema.
