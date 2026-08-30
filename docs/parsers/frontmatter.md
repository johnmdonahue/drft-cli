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

Values under a declared key, each naming a document this file derives from. With `edge_keys = ["sources", "template"]`:

```markdown
---
sources:
  - setup.md
  - ../shared/glossary.md
  - ./prior-art.md
template: docs/templates/page.md
---
```

The parser parses the YAML frontmatter block and collects every string leaf value reachable through a key you declared in [`edge_keys`](#naming-the-keys-that-yield-edges). Each one becomes an edge. Non-string types (numbers, booleans, null) are skipped — they name no target, and neither does a value that is empty or whitespace only. Only values are examined, so a mapping key inside a list (`- name: foo bar`) is not a target, though its value `foo bar` is.

**The declared key is the whole of the signal.** drft does not decide whether a value looks like a path. A value that resolves to nothing becomes an edge that resolves to nothing, and `unresolved-edge` reports it exactly as it reports a typo'd path — the remedy is yours: fix the value, fix the config, or move the field to a key you did not declare.

A frontmatter value cites another document, so — unlike a body link — a value that is only a fragment (`#overview`) names no document at all. It cannot resolve, and it is reported rather than dropped. A cross-document fragment (`other.md#section`) is unaffected: the target is `other.md`, and the fragment is kept on the occurrence.

**The finding names the target, which is resolved against the declaring file.** A value under `docs/guide.md` is reported as `docs/TBD`, not `TBD`. The literal text is on the edge's occurrence as `raw`, which `drft edges` shows and `check` does not.

Text output is line-oriented and arrow-delimited, so a target containing a newline or a control character is escaped when rendered there. JSON carries the value exactly as written and is the authoritative form.

Edges and [metadata](#metadata) read the same rendering of the block. The raw block is preferred, and the masked copy — code spans blanked — is read only when the raw block is not a YAML mapping on its own. What reaches it is any block YAML rejects until a span is blanked. A value that _begins_ with a backtick is the commonest, since the character is a reserved indicator there; a span hiding a `:` is another, and a span swallowing a line break can take a `-` or a tab into a position that breaks the mapping.

Because both read the same rendering, **an edge carries the target `@frontmatter` reports**. A code span inside a value is part of that value rather than something blanked out of it:

```yaml
sources: ./setup.md `draft`
```

That value names a target ending in `` `draft` ``, which nothing answers to, so it raises `unresolved-edge` rather than quietly resolving to `setup.md`.

One qualification: a target is recorded as resolved against the declaring file, so `./setup.md` in a block appears as `setup.md` on the edge.

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

### Naming the keys that yield edges

`edge_keys` names the frontmatter keys whose values are derivations:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
edge_keys = ["sources"]
```

Only values reachable through one of those keys become edges. A matched key hands over its whole subtree, so lists and nested maps under `sources:` still yield every value beneath them, and the key is matched at any depth — `meta.sources` is found too. Values under every other key are left alone, which is what keeps an API route (`route: /customers`) and a glob naming the files a rule governs (`paths: ["api/**"]`) out of the graph.

`edge_keys` scopes edges only — [metadata](#metadata) always captures the whole block, including the keys you did not declare.

**Omitting it is a supported shape.** A frontmatter graph may exist purely to seed node metadata, with no provenance edges at all, so a graph without `edge_keys` loads and emits none. It raises a `no-edge-keys` hint saying so, because the other reading is a repo that believes it is tracking provenance and is not. Hints are advisory and do not change the exit code.

`edge_keys = []` is the same state written out — an empty set names nowhere to look — so it behaves identically and raises the same hint.

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
edge_keys = ["sources"]
```

`files` scopes which files the parser reads (default `["**/*.md"]`); `edge_keys` names the keys whose values yield edges (see [naming the keys that yield edges](#naming-the-keys-that-yield-edges)). See [configuration](../config.md) for the full graph schema.
