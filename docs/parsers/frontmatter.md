---
purpose: the frontmatter parser — edges from declared keys plus node metadata
sources:
  - ../../src/parsers/frontmatter.rs
---

# Frontmatter parser

## The concept

The frontmatter parser extracts YAML frontmatter from files. It serves two purposes: emitting the values under declared keys as edges, and attaching the parsed frontmatter as node metadata.

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

Text output is line-oriented, so one record is always one line: a value carrying a newline or a control character is escaped wherever text renders it. The escaping belongs to the rendering — every value reaches JSON, and the lockfile, exactly as written, which is the authoritative form.

Edges and [metadata](#metadata) read one parse of the block as written. A block that cannot be read completely as a YAML mapping yields neither, and raises `unreadable-frontmatter` naming the file. This includes a literal NUL: the YAML scanner treats it as end of input, so drft rejects the whole block instead of publishing the prefix before it. A NUL in body text is outside the block and has no effect.

The commonest invalid block is a value that _begins_ with a backtick, since the character is a reserved indicator in YAML. Quote the value or write it as a block scalar:

```yaml
purpose: "`widget-loader` is the entry point"
note: |
  `widget-loader` is the entry point
```

Both are captured verbatim, backticks included. A span hiding a `:` and a span swallowing a line break break a block the same way and take the same remedy.

Because edges and metadata read one rendering, **an edge carries the target `@frontmatter` reports**. A code span inside a value is part of that value rather than something blanked out of it:

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

Frontmatter that cannot be read completely as a YAML mapping contributes no edges or metadata, and raises `unreadable-frontmatter` naming the file. drft is not a YAML linter and does not say which construct failed — only that a block it recognized could not be read, which is what separates a file that declares nothing from one whose declarations were dropped.

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

**Omitting it is a supported shape.** A frontmatter graph may exist purely to seed node metadata, with no provenance edges at all, so a graph without `edge_keys` loads, emits none, and says nothing about it. `edge_keys = []` is that same state written out — an empty set names nowhere to look — so it behaves identically.

Declaring keys states an expectation the corpus can fail to meet, and that is the state worth reporting. A graph that declares `edge_keys` and finds nothing under them that yields an edge raises an `edge-keys-matched-nothing` hint: a misspelled key otherwise produces a graph tracking nothing while the config says otherwise, at exit 0 and in silence. A graph that read no file at all says that instead, and points at the globs, the `ignore` patterns, and whether the matched files are readable text — a graph reaching nothing looks the same whether its globs are wrong or its corpus is unwritten, so the hint names both rather than guessing. Hints are advisory and do not change the exit code.

## Metadata

The parser attaches the parsed frontmatter block to the file's node. In the composed graph it nests under the graph's `@<name>` namespace — `@frontmatter` for a graph named `frontmatter` — alongside the file's `@fs` facts.

Where a block ends is decided by fence syntax, and only then is its text read. A block is opened by exactly three dashes on a line of their own and closed by a line of exactly `---` or `...`, and its first line may be neither blank nor a closing fence. These are the [markdown parser](markdown.md)'s rules, and both parsers read one decider rather than two implementations of it, because a parser claiming a span the other renders publishes the same text as metadata and as an address at once. The fence line is the fence: whitespace after the opening `---` is outside the text the YAML parser reads. Both parsers also read **only a leading block** — the markdown library is willing to claim one anywhere, and neither parser takes it. A block further down a document is prose to both, which is what every renderer shows.

Nothing about a block's content moves its boundary. A file opening with a `---` thematic break above a blank line keeps its first heading and its links, and a backtick opened in frontmatter and closed in the body changes neither where the block ends nor what it says.

Metadata is contributed only when the complete block parses as a YAML **mapping**. A block that does not — a comment-only block, one parsing to a bare scalar, malformed YAML, or one truncated by a literal NUL — contributes nothing, and raises `unreadable-frontmatter` naming the file. The rule is what makes the outcome recoverable: the block is claimed by its fences either way, so nothing renders its text as content, and silence about a block a reader can see is the failure worth preventing.

## Configuration

Declare a graph that uses the frontmatter parser:

```toml
[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
edge_keys = ["sources"]
```

`files` scopes which files the parser reads (default `["**/*.md"]`); `edge_keys` names the keys whose values yield edges (see [naming the keys that yield edges](#naming-the-keys-that-yield-edges)). See [configuration](../config.md) for the full graph schema.
