---
purpose: configure the walk, graphs, and rules through drft.toml
sources:
  - ../src/config.rs
  - ../src/cli.rs
---

# Configuration

`drft.toml` in the graph root configures the walk, the graphs, and the rules.
The directory containing `drft.toml` is the graph root; nested `drft.toml` files
found while walking are ordinary files on disk, not graph boundaries.

## ignore

```toml
ignore = ["target/**", "drafts/**"]
```

The `fs` graph walks every file under the graph root, including dot-directories
like `.github/` — only version-control stores (`.git`, `.hg`, `.svn`, `.jj`) are
pruned. `ignore` removes paths from that walk by glob; drft also respects
`.gitignore` automatically. There is no `include` — the graph is everything
under the root minus what `ignore` and `.gitignore` remove. drft excludes its
own `drft.lock` from the graph.

Only committed `.gitignore` rules count. For reproducibility, the graph never
depends on machine-local or above-root state: your global gitignore, the
per-clone `.git/info/exclude`, and ignore files in directories above the root
are all disregarded.

This top-level `ignore` is a **discovery** filter: matching paths never become
nodes, so nothing links to them and nothing is validated against them. To keep
files in the graph (so your links to them resolve and stay drift-tracked) but
skip _validating_ them, use the rule-level `ignore` instead — see
[rules](rules/README.md).

## graphs

A graph pairs a file scope (`files`) with a parser that interprets the matched
files. The `fs` graph is implicit and always built — it owns the identity space
(paths) and contributes each file's `type` and `hash`. There are no default
graphs: declare each one you want under `[graphs.<name>]`, and that set is the
whole set.

```toml
[graphs.markdown]
parser = "markdown"
files = ["**/*.md"]

[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
```

| Field    | Required | Default        | Description                                                        |
| -------- | -------- | -------------- | ------------------------------------------------------------------ |
| `parser` | yes      | —              | How to interpret the files (see [parsers](parsers/README.md))      |
| `files`  | no       | `["**/*.md"]`  | Globs scoping which files the parser reads                         |
| `keys`   | no       | shape detected | `frontmatter` only — the frontmatter keys whose values yield edges |

`keys` names what the graph tracks, so a path-shaped value under an unrelated key
(`route: /customers`) is not mistaken for a derivation. Omit it to classify by
value shape across the whole block. It applies to the `frontmatter` parser only —
`markdown` has no keyed structure — and declaring it elsewhere, or as an empty
list, is a config error. See [the frontmatter parser](parsers/frontmatter.md#scoping-to-keys).

These are the only accepted keys. Any other key is a config error naming the key
and the accepted set. A graph table that parses is read as a graph that works, so
an option drft does not support fails loudly rather than being silently discarded.

The graph's name is its compose-time namespace: its facts nest under `@<name>`
in the composed graph. A name must not contain `@`, start with `_`, or be `fs` —
all reserved. `fs` is always built without being declared, and is a provider
rather than a parser, so `parser = "fs"` is rejected too. With no `[graphs.*]`,
only the `fs` graph is built.

## rules

Every built-in rule is on at `warn`. Configure severity and ignore globs under
`[rules.<name>]`:

```toml
[rules]
ignore = ["vendor/**"] # global: suppress every rule for these subjects
stale-node = "error" # shorthand: severity only
stale-edge = "error"

[rules.detached-node] # table form: severity + ignore
severity = "off"

[rules.unresolved-edge]
ignore = ["CHANGELOG.md"] # globs matched against the finding's subject
```

The `ignore` key directly under `[rules]` is a **diagnostic** filter applied to
_every_ rule, unioned with each rule's own `ignore`. Unlike the top-level
`ignore`, matching paths stay in the graph — they still resolve links and carry
drift hashes — so a file of yours that links a suppressed one is still flagged
when that target changes (the finding's subject is your file, not the suppressed
one). Use it for whole groups you depend on but don't own: "validate my files,
not theirs." (`ignore` is a reserved key here; no rule may be named `ignore`.)

| Field      | Required | Default | Description                                     |
| ---------- | -------- | ------- | ----------------------------------------------- |
| `severity` | no       | `warn`  | `"error"`, `"warn"`, or `"off"`                 |
| `ignore`   | no       | none    | Globs — suppress findings whose subject matches |

Any other key in a `[rules.<name>]` table is a config error. An unknown rule
_name_ only warns, since both fields default and a misspelled rule would
otherwise configure nothing in silence. The warning is an `unknown-rule`
[hint](reading.md#hints), carrying the config key as its locus.

See [rules](rules/README.md) for the full set.
